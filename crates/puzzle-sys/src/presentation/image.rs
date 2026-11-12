//! IOSurface storage imported into the existing wgpu Metal device (D-60).
//!
//! Native bytes stay BGRA8Unorm, matching D-27's encoded blending. The IOSurface
//! carries an sRGB ICC profile so CALayer composition does not interpret them
//! in the display's device colour space. There is no CPU mirror or GPU readback;
//! explicit host thumbnails read completed shared bytes only on request.
//!
//! IOSurface owns the storage; the Metal texture is a view of that storage, and
//! wgpu imports that same texture rather than allocating a second image. A render
//! pass clears the full target before writing terminal ink. There is no CPU map,
//! CPU copy, GPU readback or colour conversion in ordinary presentation.
//!
//! Device limits are checked before allocation. Pixel format, plane, extent,
//! mip count, sample count and usage must agree at all three boundaries: Apple's
//! descriptor, HAL ownership and wgpu validation. Importing a mismatched texture
//! would defeat validation, so construction keeps these descriptions together.
//!
//! Colour metadata belongs to the allocation, not to the old CAMetalLayer that
//! now acts only as a container. Otherwise saturated sRGB bytes would again be
//! treated as display-native P3 when Core Animation composites the plain layer.
//! The ICC data is installed before publication and read back by the native test.
//! Framework dictionaries retain their keys and values; local construction
//! references can be dropped as soon as IOSurface creation returns.
use super::Geometry;
use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained};
use objc2_core_graphics::{CGColorSpace, kCGColorSpaceSRGB};
use objc2_io_surface::{
    IOSurfaceRef, kIOSurfaceBytesPerElement, kIOSurfaceColorSpace, kIOSurfaceHeight,
    kIOSurfacePixelFormat, kIOSurfaceWidth,
};
use objc2_metal::{
    MTLDevice, MTLPixelFormat, MTLStorageMode, MTLTextureDescriptor, MTLTextureType,
    MTLTextureUsage,
};

/// One immutable native allocation and its renderer-owned texture view.
pub(super) struct Image {
    /// Retains backing storage independently of Metal and Core Animation's references.
    pub surface: CFRetained<IOSurfaceRef>,
    /// wgpu validates and tracks this same imported Metal allocation.
    pub texture: wgpu::Texture,
    /// Physical allocation capacity; per-frame geometry lives with the submission.
    pub size: [u32; 2],
}
// SAFETY: IOSurface retain/release and use-count queries are thread-safe. We never
// expose its mutable CPU address or change its properties after construction.
// Metal writes are completed before publication; CALayer access stays on main.
#[allow(
    unsafe_code,
    reason = "R8/D-60: retained IOSurface crosses GPU completion and main threads"
)]
// SAFETY: the immutable resource and callback contract is documented above.
unsafe impl Send for Image {}
// SAFETY: shared access exposes immutable metadata/completed pixels only. Slot
// admission excludes simultaneous Metal writes and compositor/thumbnail reads.
#[allow(
    unsafe_code,
    reason = "R8/D-60: immutable IOSurface ownership is shared across presentation"
)]
// SAFETY: shared readers use atomics/retained ownership; writes have one owner.
unsafe impl Sync for Image {}

/// Construction is cold: new geometry/device creates at most one bounded pool.
impl Image {
    /// Allocates BGRA storage on this device, with matching wgpu/HAL descriptors.
    #[allow(unsafe_code, reason = "R8/D-60: same-device IOSurface texture import")]
    pub(super) fn new(device: &wgpu::Device, geometry: Geometry) -> Result<Self, String> {
        let [width, height] = geometry.size;
        let limit = device.limits().max_texture_dimension_2d;
        if width == 0 || height == 0 || width > limit || height > limit {
            return Err("IOSurface dimensions outside device limits".into());
        }
        let started = tracing::enabled!(target:"puzzle_desktop::resize", tracing::Level::TRACE)
            .then(std::time::Instant::now);
        let surface = allocate(geometry)?;
        let descriptor = MTLTextureDescriptor::new();
        descriptor.setTextureType(MTLTextureType::Type2D);
        descriptor.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
        // SAFETY: width is nonzero and within this device's validated limit.
        unsafe { descriptor.setWidth(width as usize) };
        // SAFETY: height is nonzero and within this device's validated limit.
        unsafe { descriptor.setHeight(height as usize) };
        descriptor.setStorageMode(MTLStorageMode::Shared);
        descriptor.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
        // SAFETY: the device is borrowed for the guard; no native device/queue
        // ownership is changed or destroyed through this handle.
        let hal =
            unsafe { device.as_hal::<wgpu::hal::api::Metal>() }.ok_or("IOSurface needs Metal")?;
        let raw = hal
            .raw_device()
            .newTextureWithDescriptor_iosurface_plane(&descriptor, &surface, 0)
            .ok_or("Metal rejected IOSurface texture")?;
        let extent = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        // SAFETY: native texture was created on this exact device, as one BGRA
        // 2D layer/mip of extent. Ownership transfers to HAL; no alias is destroyed.
        let texture = unsafe {
            wgpu::hal::metal::Device::texture_from_raw(
                raw,
                wgpu::TextureFormat::Bgra8Unorm,
                MTLTextureType::Type2D,
                1,
                1,
                extent.into(),
                None,
            )
        };
        let desc = wgpu::TextureDescriptor {
            label: Some("terminal IOSurface"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        };
        // SAFETY: desc matches the native texture and this device. Discard its
        // initial contents; the ordinary render pass clears every attachment.
        let texture = unsafe {
            device.create_texture_from_hal::<wgpu::hal::api::Metal>(
                texture,
                &desc,
                wgpu::TextureUses::UNINITIALIZED,
            )
        };
        // One event accounts for both the IOSurface and its imported Metal view.
        trace_allocation(started, geometry);
        Ok(Self {
            surface,
            texture,
            size: geometry.size,
        })
    }
}

/// Uses framework keys, with equal-length fixed property arrays and no raw CF FFI.
#[allow(
    unsafe_code,
    reason = "R8/D-60: IOSurface creation properties and immutable framework names"
)]
fn allocate(geometry: Geometry) -> Result<CFRetained<IOSurfaceRef>, String> {
    // SAFETY: these immutable framework CFString references have process lifetime.
    let keys = unsafe {
        [
            kIOSurfaceWidth,
            kIOSurfaceHeight,
            kIOSurfaceBytesPerElement,
            kIOSurfacePixelFormat,
        ]
    };
    // FourCC 'BGRA' is Apple's IOSurface/Metal BGRA8 pixel format, not RGBA bytes.
    let values = [
        geometry.size[0],
        geometry.size[1],
        4,
        u32::from_be_bytes(*b"BGRA"),
    ]
    .map(|n| CFNumber::new_i64(i64::from(n)));
    let values = values.each_ref().map(|n| &**n);
    let properties = CFDictionary::from_slices(&keys, &values);
    // SAFETY: all keys/values have their documented CFString/CFNumber types;
    // dimensions were validated and both fixed arrays contain exactly four entries.
    let surface = unsafe { IOSurfaceRef::new(properties.as_opaque()) }
        .ok_or("IOSurface allocation failed")?;
    // SAFETY: the CoreGraphics colour-space name is immutable and always available.
    let name = unsafe { kCGColorSpaceSRGB };
    let space = CGColorSpace::with_name(Some(name)).ok_or("cannot create sRGB colour space")?;
    let profile = CGColorSpace::icc_data(Some(&space)).ok_or("cannot create sRGB ICC profile")?;
    // SAFETY: immutable key denotes an ICC profile; its lifetime is the process.
    let key = unsafe { kIOSurfaceColorSpace };
    // SAFETY: value is an ICC CFData; IOSurface copies/retains it. Metadata is set
    // only before the allocation is exposed to Metal, a worker or Core Animation.
    unsafe { surface.set_value(key, profile.as_ref()) };
    Ok(surface)
}

/// Reads the actual IOSurface ICC attachment; absent/device-space content fails.
#[allow(unsafe_code, reason = "R8/D-60: immutable IOSurface ICC property name")]
pub(super) fn is_srgb(surface: &IOSurfaceRef) -> bool {
    // SAFETY: Apple's immutable property name has process lifetime.
    let key = unsafe { kIOSurfaceColorSpace };
    let Some(data) = surface
        .value(key)
        .and_then(|v| v.downcast::<objc2_core_foundation::CFData>().ok())
    else {
        return false;
    };
    // SAFETY: downcast above verifies the CFData type required by this API.
    let Some(space) = (unsafe { CGColorSpace::with_icc_data(Some(data.as_ref())) }) else {
        return false;
    };
    // SAFETY: the immutable CoreGraphics sRGB identifier has process lifetime.
    let name = unsafe { kCGColorSpaceSRGB };
    CGColorSpace::with_name(Some(name)).is_some_and(|expected| *expected == *space)
}

/// Trace formatting stays outside texture construction and ordinary builds read no clock.
fn trace_allocation(started: Option<std::time::Instant>, geometry: Geometry) {
    if let Some(started) = started {
        tracing::trace!(target:"puzzle_desktop::resize",event="native.contents_allocated",?geometry,
            wall_ms=started.elapsed().as_secs_f64()*1000.);
    }
}
