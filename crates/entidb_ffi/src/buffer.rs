//! Buffer types for FFI.

use std::ffi::CString;

/// A byte buffer for FFI.
///
/// Memory is owned by Rust. Call `entidb_free_buffer` to release.
#[repr(C)]
pub struct EntiDbBuffer {
    /// Pointer to data.
    pub data: *mut u8,
    /// Length in bytes.
    pub len: usize,
    /// Capacity (for internal use).
    pub capacity: usize,
}

impl EntiDbBuffer {
    /// Creates a new buffer from a Vec.
    pub fn from_vec(vec: Vec<u8>) -> Self {
        let mut vec = vec.into_boxed_slice();
        let data = vec.as_mut_ptr();
        let len = vec.len();
        std::mem::forget(vec);

        Self {
            data,
            len,
            capacity: len,
        }
    }

    /// Creates an empty buffer.
    pub fn empty() -> Self {
        Self {
            data: std::ptr::null_mut(),
            len: 0,
            capacity: 0,
        }
    }

    /// Returns true if the buffer is null/empty.
    pub fn is_null(&self) -> bool {
        self.data.is_null()
    }

    /// Converts back to a Vec, consuming the buffer.
    ///
    /// # Safety
    ///
    /// The buffer must have been created from a Vec.
    pub unsafe fn into_vec(self) -> Vec<u8> {
        if self.data.is_null() {
            return Vec::new();
        }
        Vec::from_raw_parts(self.data, self.len, self.capacity)
    }
}

/// Frees a buffer allocated by EntiDB.
///
/// # Safety
///
/// The buffer must have been allocated by EntiDB FFI functions.
#[no_mangle]
pub unsafe extern "C" fn entidb_free_buffer(buffer: EntiDbBuffer) {
    if !buffer.data.is_null() {
        drop(Vec::from_raw_parts(buffer.data, buffer.len, buffer.capacity));
    }
}

/// A string for FFI.
///
/// Null-terminated UTF-8 string. Memory owned by Rust.
/// Call `entidb_free_string` to release.
#[repr(C)]
pub struct EntiDbString {
    /// Pointer to null-terminated string.
    pub ptr: *mut std::ffi::c_char,
    /// Length (not including null terminator).
    pub len: usize,
}

impl EntiDbString {
    /// Creates a new FFI string from a Rust string.
    pub fn from_str(s: &str) -> Option<Self> {
        let cstring = CString::new(s).ok()?;
        let len = cstring.as_bytes().len();
        let ptr = cstring.into_raw();

        Some(Self { ptr, len })
    }

    /// Creates an empty string.
    pub fn empty() -> Self {
        Self {
            ptr: std::ptr::null_mut(),
            len: 0,
        }
    }

    /// Returns true if the string is null.
    pub fn is_null(&self) -> bool {
        self.ptr.is_null()
    }

    /// Converts to a Rust string slice.
    ///
    /// # Safety
    ///
    /// The pointer must be valid.
    pub unsafe fn as_str(&self) -> Option<&str> {
        if self.ptr.is_null() {
            return None;
        }
        let cstr = std::ffi::CStr::from_ptr(self.ptr);
        cstr.to_str().ok()
    }
}

/// Frees a string allocated by EntiDB.
///
/// # Safety
///
/// The string must have been allocated by EntiDB FFI functions.
#[no_mangle]
pub unsafe extern "C" fn entidb_free_string(string: EntiDbString) {
    if !string.ptr.is_null() {
        drop(CString::from_raw(string.ptr));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_from_vec() {
        let data = vec![1u8, 2, 3, 4, 5];
        let buffer = EntiDbBuffer::from_vec(data.clone());

        assert!(!buffer.is_null());
        assert_eq!(buffer.len, 5);

        // Safety: we just created it
        let recovered = unsafe { buffer.into_vec() };
        assert_eq!(recovered, data);
    }

    #[test]
    fn buffer_empty() {
        let buffer = EntiDbBuffer::empty();
        assert!(buffer.is_null());
        assert_eq!(buffer.len, 0);
    }

    #[test]
    fn string_from_str() {
        let string = EntiDbString::from_str("hello").unwrap();
        assert!(!string.is_null());
        assert_eq!(string.len, 5);

        // Safety: we just created it
        let s = unsafe { string.as_str() };
        assert_eq!(s, Some("hello"));

        // Free it
        unsafe { entidb_free_string(string) };
    }

    #[test]
    fn string_empty() {
        let string = EntiDbString::empty();
        assert!(string.is_null());
    }

    #[test]
    fn string_with_null_byte_fails() {
        let result = EntiDbString::from_str("hello\0world");
        assert!(result.is_none());
    }
}
