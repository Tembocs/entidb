//! Error types for WASM bindings.

use thiserror::Error;
use wasm_bindgen::prelude::*;

/// Errors that can occur in the WASM bindings.
#[derive(Debug, Error)]
pub enum WasmError {
    /// Storage operation failed.
    #[error("storage error: {0}")]
    Storage(String),

    /// Database operation failed.
    #[error("database error: {0}")]
    Database(String),

    /// Invalid input provided.
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// JavaScript error occurred.
    #[error("JS error: {0}")]
    JsError(String),

    /// Feature not supported in current browser.
    #[error("not supported: {0}")]
    NotSupported(String),

    /// OPFS operation failed.
    #[error("OPFS error: {0}")]
    Opfs(String),

    /// IndexedDB operation failed.
    #[error("IndexedDB error: {0}")]
    IndexedDb(String),
}

impl From<WasmError> for JsValue {
    fn from(err: WasmError) -> Self {
        JsValue::from_str(&err.to_string())
    }
}

impl From<JsValue> for WasmError {
    fn from(val: JsValue) -> Self {
        WasmError::JsError(
            val.as_string()
                .unwrap_or_else(|| format!("{:?}", val)),
        )
    }
}

impl From<entidb_storage::StorageError> for WasmError {
    fn from(err: entidb_storage::StorageError) -> Self {
        WasmError::Storage(err.to_string())
    }
}

impl From<entidb_core::CoreError> for WasmError {
    fn from(err: entidb_core::CoreError) -> Self {
        WasmError::Database(err.to_string())
    }
}

/// Result type for WASM operations.
pub type WasmResult<T> = Result<T, WasmError>;
