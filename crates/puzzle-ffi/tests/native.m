// AppKit fixture supplies real host-owned NSViews; this is not shipped host UI.
#import <AppKit/AppKit.h>
#import <QuartzCore/QuartzCore.h>
#import <IOSurface/IOSurface.h>
#include <stdint.h>

// Each returned view is retained independently until the C test observes release.
void *puzzle_test_view_new(void) {
    [NSApplication sharedApplication];
    [NSApp setActivationPolicy:NSApplicationActivationPolicyAccessory];
    static BOOL launched = NO;
    if (!launched) { [NSApp finishLaunching]; launched = YES; }
    // Place the witness on a currently active display, including clamshell mode.
    NSRect visible = NSScreen.mainScreen.visibleFrame;
    NSRect frame = NSMakeRect(NSMidX(visible) - 200, NSMidY(visible) - 120, 400, 240);
    NSWindow *window = [[NSWindow alloc] initWithContentRect:frame
        styleMask:NSWindowStyleMaskTitled backing:NSBackingStoreBuffered defer:NO];
    [window setReleasedWhenClosed:NO];
    // The first-frame contract needs an actual drawable. Ordinary orderFront
    // can leave a command-line fixture below the foreground app and occluded.
    [window orderFrontRegardless];
    return (__bridge_retained void *)window;
}
// NSView validity is tied to its retained window fixture, never to a stack object.
void *puzzle_test_view(void *handle) {
    return (__bridge NSWindow *)handle == nil ? NULL : (__bridge void *)[(__bridge NSWindow *)handle contentView];
}
// Display identity is read at runtime; this test does not invent a 120-Hz witness.
uint64_t puzzle_test_display(void *handle) {
    NSScreen *screen = [(__bridge NSWindow *)handle screen];
    return [[[screen deviceDescription] objectForKey:@"NSScreenNumber"] unsignedLongLongValue];
}
// Pump only the host event loop; Puzzle worker callbacks merely set a wake flag.
void puzzle_test_pump(void) {
    // Dispatch AppKit events too: merely running CFRunLoop leaves window-server
    // occlusion changes queued, which makes Metal reject every drawable.
    NSEvent *event = [NSApp nextEventMatchingMask:NSEventMaskAny
        untilDate:[NSDate dateWithTimeIntervalSinceNow:0.005]
        inMode:NSDefaultRunLoopMode dequeue:YES];
    if (event) [NSApp sendEvent:event];
}
// This is called only after the Puzzle native-target lifetime barrier.
void puzzle_test_view_free(void *handle) {
    NSWindow *window = (__bridge_transfer NSWindow *)handle;
    [window close];
}

// The same retained fixture can supply its own layer instead of asking wgpu to create one.
void *puzzle_test_layer(void *handle) {
    NSView *view = [(__bridge NSWindow *)handle contentView];
    CAMetalLayer *layer = [CAMetalLayer layer];
    // Deliberately start with the scaling default: Puzzle must apply D-54 itself.
    layer.contentsGravity = kCAGravityResize;
    view.layer = layer;
    view.wantsLayer = YES;
    return (__bridge void *)layer;
}
// Readback proves surface configuration preserves the host's top-left contract.
int puzzle_test_layer_is_top_left(void *layer) {
    return [((__bridge CAMetalLayer *)layer).contentsGravity isEqualToString:kCAGravityTopLeft];
}
// IOSurface contents are observed on the actual Puzzle-owned presentation layer.
static IOSurfaceRef image_contents(CALayer *root) {
    if (root.contents && CFGetTypeID((__bridge CFTypeRef)root.contents) == IOSurfaceGetTypeID())
        return (__bridge IOSurfaceRef)root.contents;
    for (CALayer *child in root.sublayers) {
        IOSurfaceRef found = image_contents(child);
        if (found) return found;
    }
    return NULL;
}
// Search through wgpu's NSView-created Metal container too.
static CALayer *presenter(CALayer *root);
// Query Puzzle's own child, not the host container or a sampled framebuffer.
int puzzle_test_background(void *window, void *pointer, unsigned r, unsigned g, unsigned b) {
    CALayer *root = pointer ? (__bridge CALayer *)pointer : [(__bridge NSWindow *)window contentView].layer;
    CALayer *child = presenter(root);
    CGColorRef color = child.backgroundColor;
    if (!color || CGColorGetNumberOfComponents(color) != 4) return NO;
    CGColorSpaceRef space = CGColorGetColorSpace(color);
    CFStringRef name = space ? CGColorSpaceCopyName(space) : NULL;
    BOOL srgb = name && CFEqual(name, kCGColorSpaceSRGB);
    if (name) CFRelease(name);
    const CGFloat *rgba = CGColorGetComponents(color);
    return srgb && fabs(rgba[0]*255-r)<0.00001 && fabs(rgba[1]*255-g)<0.00001 &&
        fabs(rgba[2]*255-b)<0.00001 && rgba[3] == 1;
}
// Host bounds are set first, just as AppKit does before surface_resize.
void puzzle_test_geometry(void *handle, uint32_t width, uint32_t height, double scale) {
    NSWindow *window = (__bridge NSWindow *)handle;
    [window setContentSize:NSMakeSize(width / scale, height / scale)];
    [window.contentView layoutSubtreeIfNeeded];
}
// The real presenter is a CALayer child; the host container identity stays fixed.
static CALayer *presenter(CALayer *root) {
    if ([NSStringFromClass(root.class) containsString:@"PresentationLayer"]) return root;
    for (CALayer *child in root.sublayers) { CALayer *found = presenter(child); if (found) return found; }
    return nil;
}
// Rounded allocations expose only their rendered source rectangle. Compare
// visible device pixels, never allocation capacity, to the host geometry.
static BOOL contents_size(CALayer *root, uint32_t width, uint32_t height) {
    CALayer *child = presenter(root);
    IOSurfaceRef image = child ? image_contents(child) : NULL;
    if (!image) return NO;
    CGRect crop = child.contentsRect;
    NSCAssert(crop.origin.x == 0 && crop.origin.y == 0 &&
        crop.size.width > 0 && crop.size.width <= 1 &&
        crop.size.height > 0 && crop.size.height <= 1, @"invalid top-left image crop");
    // Capacity buckets are powers of two here: the source-pixel products are
    // exact integers. A fractional pixel must fail, not disappear through rounding.
    return IOSurfaceGetWidth(image) * crop.size.width == width &&
        IOSurfaceGetHeight(image) * crop.size.height == height;
}
// Invoke CA display in the geometry transaction without pumping Puzzle tick.
// This is the path a modal resize/abrupt Magnet change must be able to complete.
int puzzle_test_contents_display(void *window, void *pointer, uint32_t width, uint32_t height, double scale) {
    CALayer *root = pointer ? (__bridge CALayer *)pointer : [(__bridge NSWindow *)window contentView].layer;
    [root layoutIfNeeded];
    CALayer *child = presenter(root);
    NSCAssert(child, @"missing IOSurface presentation layer");
    NSCAssert((id)[child actionForKey:@"contents"] == [NSNull null] &&
        (id)[child actionForKey:@"bounds"] == [NSNull null], @"presentation must explicitly reject implicit actions");
    NSTimeInterval started = NSProcessInfo.processInfo.systemUptime;
    [child display];
    NSTimeInterval waited = NSProcessInfo.processInfo.systemUptime - started;
    IOSurfaceRef image = image_contents(child);
    size_t actual_width = image ? lround(IOSurfaceGetWidth(image) * child.contentsRect.size.width) : 0;
    size_t actual_height = image ? lround(IOSurfaceGetHeight(image) * child.contentsRect.size.height) : 0;
    BOOL corner = [child.contentsGravity isEqualToString:child.contentsAreFlipped ? kCAGravityBottomLeft : kCAGravityTopLeft];
    NSCAssert(corner, @"stale contents must stay at the effective visual top left");
    NSCAssert(lround(child.bounds.size.width * child.contentsScale) == width &&
        lround(child.bounds.size.height * child.contentsScale) == height && child.contentsScale == scale,
        @"presentation geometry must use actual bounds and the forwarded backing scale");
    fprintf(stdout, "M12_LAYER_COMMIT {\"requested\":[%u,%u],\"actual\":[%zu,%zu],\"scale\":%.1f,\"wait_ms\":%.6f,\"top_left\":true,\"live_resize\":%s}\n",
        width,height,actual_width,actual_height,scale,waited*1000,[(__bridge NSWindow *)window inLiveResize] ? "true" : "false");
    fflush(stdout);
    return contents_size(root, width, height);
}
// Read the current IOSurface allocation without acquiring any drawable.
int puzzle_test_layer_size(void *layer, uint32_t width, uint32_t height) {
    return contents_size((__bridge CALayer *)layer, width, height);
}
// Transfer the same native view/layer to another window, as a tab tear-off does.
void *puzzle_test_transfer(void *handle, void *expected_layer) {
    NSWindow *source = (__bridge NSWindow *)handle;
    NSView *view = source.contentView;
    source.contentView = [[NSView alloc] initWithFrame:view.frame];
    void *destination = puzzle_test_view_new();
    NSWindow *window = (__bridge NSWindow *)destination;
    window.contentView = view;
    [window orderFrontRegardless];
    NSCAssert((__bridge void *)view.layer == expected_layer, @"host layer replaced during transfer");
    return destination;
}

// Observe the configured layer itself, including wgpu's NSView-created sublayer.
static CAMetalLayer *metal_layer(CALayer *root) {
    if ([root isKindOfClass:CAMetalLayer.class]) return (CAMetalLayer *)root;
    for (CALayer *child in root.sublayers) {
        CAMetalLayer *found = metal_layer(child);
        if (found) return found;
    }
    return nil;
}
// A host's pre-existing wide-gamut metadata must not reinterpret encoded sRGB ink.
void puzzle_test_layer_wide_gamut(void *window, void *pointer) {
    CAMetalLayer *layer = pointer ? (__bridge CAMetalLayer *)pointer
        : metal_layer([(__bridge NSWindow *)window contentView].layer);
    CGColorSpaceRef colour = CGColorSpaceCreateWithName(kCGColorSpaceDisplayP3);
    layer.colorspace = colour;
    CGColorSpaceRelease(colour);
}
// Nil disables explicit compositor matching; only the named sRGB profile passes.
int puzzle_test_layer_srgb(void *window, void *pointer, unsigned kind, const char *phase) {
    CALayer *root = pointer ? (__bridge CALayer *)pointer : [(__bridge NSWindow *)window contentView].layer;
    IOSurfaceRef image = image_contents(root);
    CFDataRef data = image ? IOSurfaceCopyValue(image, kIOSurfaceColorSpace) : NULL;
    CGColorSpaceRef colour = data ? CGColorSpaceCreateWithICCData(data) : NULL;
    CGColorSpaceRef expected = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
    int srgb = colour && CFEqual(colour, expected);
    unsigned format = image && IOSurfaceGetPixelFormat(image) == 'BGRA' ? 80 : 0;
    fprintf(stdout, "M18_LAYER_COLOUR {\"kind\":%u,\"phase\":\"%s\",\"pixel_format\":%u,\"colorspace\":\"%s\",\"edr\":0}\n",
        kind, phase, format, srgb ? "sRGB" : "absent/other");
    fflush(stdout);
    if (colour) CGColorSpaceRelease(colour);
    CGColorSpaceRelease(expected);
    if (data) CFRelease(data);
    return srgb && format == 80;
}
// Read the presented IOSurface rather than an unused CAMetalLayer drawable queue.
int puzzle_test_colour_extent(void *window, void *pointer, uint32_t width, uint32_t height) {
    CALayer *root = pointer ? (__bridge CALayer *)pointer : [(__bridge NSWindow *)window contentView].layer;
    return contents_size(root, width, height);
}
// Match the view bounds to the C geometry, as a real host does before forwarding resize.
void puzzle_test_colour_resize(void *window, uint32_t width, uint32_t height) {
    [(__bridge NSWindow *)window setContentSize:NSMakeSize(width / 2.0, height / 2.0)];
}

// Fractional split panes live inside an integer-sized window, as in the app.
void *puzzle_test_split_layer(void *handle, uint32_t width, uint32_t height, double scale) {
    NSWindow *window = (__bridge NSWindow *)handle;
    [window setContentSize:NSMakeSize(1200, 1300)];
    NSView *pane = [[NSView alloc] initWithFrame:NSMakeRect(0, 0, width/scale, height/scale)];
    pane.layer = [CAMetalLayer layer]; pane.wantsLayer = YES;
    [window.contentView addSubview:pane];
    return (__bridge void *)pane.layer;
}
// Only host-owned view geometry changes; tests never round or resize Puzzle's child.
void puzzle_test_split_resize(void *handle, void *pointer, uint32_t width, uint32_t height, double scale) {
    NSView *root = [(__bridge NSWindow *)handle contentView];
    for (NSView *pane in root.subviews) if ((__bridge void *)pane.layer == pointer) {
        [pane setFrameSize:NSMakeSize(width/scale, height/scale)];
        [root layoutSubtreeIfNeeded]; return;
    }
    NSCAssert(NO, @"split container missing");
}
// Equality is exact: half-point extents at 2x must not become whole points.
int puzzle_test_child_geometry(void *pointer, uint32_t width, uint32_t height, double scale) {
    CALayer *root = (__bridge CALayer *)pointer;
    [root layoutIfNeeded];
    CALayer *child = presenter(root);
    return child && child.bounds.size.width == width/scale &&
        child.bounds.size.height == height/scale && child.contentsScale == scale;
}

// Simulate a CA transaction retaining different bounds after host geometry arrived.
void puzzle_test_stale_container(void *pointer) {
    CALayer *root = (__bridge CALayer *)pointer;
    CGRect stale = root.bounds; stale.size.width += 1; stale.size.height += 1;
    root.bounds = stale;
    [root layoutIfNeeded];
}
