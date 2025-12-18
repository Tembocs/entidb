//! Utility functions for WASM setup.

/// Sets up the panic hook for better error messages.
///
/// This function is called automatically when the WASM module initializes.
/// It redirects Rust panic messages to the browser console for debugging.
pub fn set_panic_hook() {
    // When the `console_error_panic_hook` feature is enabled, we can call the
    // `set_panic_hook` function to get better error messages on panics.
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}
