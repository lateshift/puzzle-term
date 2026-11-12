//! Audited pointer/length borrowing at the C boundary (D-43/R8).
//!
//! We reject null, misaligned and overflowing ranges before accessing memory.
//! C still owns validity: a nonnull pointer must address the declared live
//! readable/writable allocation. Borrowed inputs never survive the call.
//! MIME arrays share the same checked foreign-range contract as byte buffers.
//! Element alignment and total address range are checked before building a slice.
use crate::abi::{Bytes, ResultCode};

/// Validates an object pointer without constructing a Rust reference.
pub(crate) fn pointer<T>(data: *const T) -> Result<(), ResultCode> {
    if data.is_null()
        || !(data as usize).is_multiple_of(align_of::<T>())
        || (data as usize).checked_add(size_of::<T>()).is_none()
    {
        return Err(ResultCode::InvalidArgument);
    }
    Ok(())
}
/// Copies a C structure rather than retaining a reference across callbacks.
#[doc = "# Safety"]
/// Nonnull data must point to an initialized readable T for this call.
#[allow(unsafe_code, reason = "D-43: checked C argument copy")]
pub(crate) unsafe fn read<T: Copy>(data: *const T) -> Result<T, ResultCode> {
    // Pointer shape is checked before creating a reference or touching the allocation.
    pointer(data)?;
    // SAFETY: pointer checked alignment/null/overflow; caller promises a live initialized T.
    Ok(unsafe { data.read() })
}
/// Writes an output only after validating the complete destination range.
#[doc = "# Safety"]
/// Nonnull data must point to writable storage for one T, with no aliasing borrow.
#[allow(unsafe_code, reason = "R8/D-43: C output initialization")]
pub(crate) unsafe fn write<T>(data: *mut T, value: T) -> Result<(), ResultCode> {
    pointer(data)?;
    // SAFETY: validation precedes the write; caller supplies live writable T storage.
    unsafe { data.write(value) };
    Ok(())
}
/// Borrows bytes without dereferencing an empty buffer, even when its pointer is null.
#[doc = "# Safety"]
/// A nonempty buffer must be readable and unmodified for the chosen borrow lifetime.
#[allow(unsafe_code, reason = "D-43: sole C slice construction")]
pub(crate) unsafe fn bytes<'a>(data: Bytes) -> Result<&'a [u8], ResultCode> {
    // Rust slices require nonnull addresses even when empty; avoid foreign pointers entirely.
    if data.len == 0 {
        return Ok(&[]);
    }
    // A single allocation cannot exceed Rust's pointer-offset range or wrap an address.
    if data.len > isize::MAX as usize
        || data.data.is_null()
        || (data.data as usize).checked_add(data.len).is_none()
    {
        return Err(ResultCode::InvalidArgument);
    }
    // SAFETY: nonempty/null/overflow bounds checked; caller promises one live readable allocation.
    Ok(unsafe { std::slice::from_raw_parts(data.data, data.len) })
}
/// Borrow a caller-owned output without retaining it past the exported call.
///
#[doc = "# Safety"]
/// Nonempty data names one writable allocation, exclusive for this borrow.
#[allow(unsafe_code, reason = "D-43: checked writable C byte range")]
pub(crate) unsafe fn bytes_mut<'a>(data: *mut u8, len: usize) -> Result<&'a mut [u8], ResultCode> {
    if len == 0 {
        return Ok(&mut []);
    }
    pointer(data)?;
    if len > isize::MAX as usize || (data as usize).checked_add(len).is_none() {
        return Err(ResultCode::InvalidArgument);
    }
    // SAFETY: bounds/null/overflow were checked; caller guarantees exclusive
    // writable storage. No reference or foreign pointer survives the call.
    Ok(unsafe { std::slice::from_raw_parts_mut(data, len) })
}
/// UTF-8 is validated before an owned string can enter Session messages.
#[doc = "# Safety"]
/// The Bytes readable-memory contract applies only until this function returns.
#[allow(
    unsafe_code,
    reason = "D-43: UTF-8 input borrows the audited slice helper"
)]
pub(crate) unsafe fn text(data: Bytes) -> Result<String, ResultCode> {
    // SAFETY: forwarded caller contract; the borrowed slice is copied before returning.
    let data = unsafe { bytes(data)? };
    std::str::from_utf8(data)
        .map(str::to_owned)
        .map_err(|_| ResultCode::InvalidArgument)
}
/// Callback slices are valid exactly while their owned backing event remains borrowed.
pub(crate) fn borrowed(data: &[u8]) -> Bytes {
    Bytes {
        data: data.as_ptr(),
        len: data.len(),
    }
}

#[cfg(test)]
/// Miri exercises empty/null/invalid UTF-8 and oversized ranges before dereference.
mod tests;

/// Borrows a bounded C array without creating a reference to a null empty range.
///
/// # Safety
/// A nonempty range names `count` initialized readable T values for this borrow.
#[allow(unsafe_code, reason = "D-43: sole checked C array slice construction")]
pub(crate) unsafe fn array<'a, T>(data: *const T, count: usize) -> Result<&'a [T], ResultCode> {
    if count == 0 {
        return Ok(&[]);
    }
    pointer(data)?;
    let bytes = count
        .checked_mul(size_of::<T>())
        .ok_or(ResultCode::InvalidArgument)?;
    if bytes > isize::MAX as usize || (data as usize).checked_add(bytes).is_none() {
        return Err(ResultCode::InvalidArgument);
    }
    // SAFETY: shape/alignment/overflow checked; caller guarantees initialized readable values.
    Ok(unsafe { std::slice::from_raw_parts(data, count) })
}
