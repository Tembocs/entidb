//! Error codes and result types.

use std::cell::RefCell;
use std::ffi::CString;

/// Result code for FFI functions.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntiDbResult {
    /// Operation succeeded.
    Ok = 0,
    /// Generic error.
    Error = 1,
    /// Invalid argument.
    InvalidArgument = 2,
    /// Entity not found.
    NotFound = 3,
    /// Transaction conflict.
    Conflict = 4,
    /// Database is closed.
    Closed = 5,
    /// Database is locked.
    Locked = 6,
    /// Corruption detected.
    Corruption = 7,
    /// I/O error.
    IoError = 8,
    /// Out of memory.
    OutOfMemory = 9,
    /// Invalid format.
    InvalidFormat = 10,
    /// Codec error.
    CodecError = 11,
    /// Null pointer.
    NullPointer = 12,
    /// Feature not supported.
    NotSupported = 13,
}

impl EntiDbResult {
    /// Returns true if the result indicates success.
    pub fn is_ok(self) -> bool {
        self == EntiDbResult::Ok
    }

    /// Returns true if the result indicates an error.
    pub fn is_err(self) -> bool {
        self != EntiDbResult::Ok
    }
}

/// Error code type for C compatibility.
pub type ErrorCode = i32;

impl From<EntiDbResult> for ErrorCode {
    fn from(result: EntiDbResult) -> Self {
        result as ErrorCode
    }
}

impl From<ErrorCode> for EntiDbResult {
    fn from(code: ErrorCode) -> Self {
        match code {
            0 => EntiDbResult::Ok,
            1 => EntiDbResult::Error,
            2 => EntiDbResult::InvalidArgument,
            3 => EntiDbResult::NotFound,
            4 => EntiDbResult::Conflict,
            5 => EntiDbResult::Closed,
            6 => EntiDbResult::Locked,
            7 => EntiDbResult::Corruption,
            8 => EntiDbResult::IoError,
            9 => EntiDbResult::OutOfMemory,
            10 => EntiDbResult::InvalidFormat,
            11 => EntiDbResult::CodecError,
            12 => EntiDbResult::NullPointer,
            13 => EntiDbResult::NotSupported,
            _ => EntiDbResult::Error,
        }
    }
}

// Thread-local storage for last error message
thread_local! {
    static LAST_ERROR: RefCell<Option<CString>> = const { RefCell::new(None) };
}

/// Sets the last error message.
pub fn set_last_error(message: impl Into<String>) {
    let msg = message.into();
    LAST_ERROR.with(|e| {
        *e.borrow_mut() = CString::new(msg).ok();
    });
}

/// Clears the last error.
pub fn clear_last_error() {
    LAST_ERROR.with(|e| {
        *e.borrow_mut() = None;
    });
}

/// Gets the last error message as a C string.
///
/// Returns null if no error is set.
///
/// # Safety
///
/// The returned pointer is valid until the next FFI call on this thread.
#[no_mangle]
pub extern "C" fn entidb_get_last_error() -> *const std::ffi::c_char {
    LAST_ERROR.with(|e| {
        match e.borrow().as_ref() {
            Some(cstr) => cstr.as_ptr(),
            None => std::ptr::null(),
        }
    })
}

/// Clears the last error message.
#[no_mangle]
pub extern "C" fn entidb_clear_error() {
    clear_last_error();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_codes() {
        assert_eq!(EntiDbResult::Ok as i32, 0);
        assert_eq!(EntiDbResult::Error as i32, 1);
        assert!(EntiDbResult::Ok.is_ok());
        assert!(EntiDbResult::Error.is_err());
    }

    #[test]
    fn error_code_conversion() {
        let result = EntiDbResult::NotFound;
        let code: ErrorCode = result.into();
        assert_eq!(code, 3);

        let back: EntiDbResult = code.into();
        assert_eq!(back, EntiDbResult::NotFound);
    }

    #[test]
    fn last_error() {
        clear_last_error();
        assert!(entidb_get_last_error().is_null());

        set_last_error("test error");
        let ptr = entidb_get_last_error();
        assert!(!ptr.is_null());

        // Safety: we just set it
        let msg = unsafe { std::ffi::CStr::from_ptr(ptr) };
        assert_eq!(msg.to_str().unwrap(), "test error");

        clear_last_error();
        assert!(entidb_get_last_error().is_null());
    }
}
