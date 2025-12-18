//! Error types for the codec crate.

use thiserror::Error;

/// Result type for codec operations.
pub type CodecResult<T> = Result<T, CodecError>;

/// Errors that can occur during encoding or decoding.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum CodecError {
    /// Failed to encode value to CBOR.
    #[error("encoding failed: {message}")]
    EncodingFailed {
        /// Description of the encoding error.
        message: String,
    },

    /// Failed to decode CBOR bytes.
    #[error("decoding failed: {message}")]
    DecodingFailed {
        /// Description of the decoding error.
        message: String,
    },

    /// Float values are forbidden in canonical CBOR.
    #[error("float values are forbidden in canonical CBOR")]
    FloatForbidden,

    /// NaN values are forbidden.
    #[error("NaN values are forbidden")]
    NaNForbidden,

    /// Indefinite-length items are forbidden.
    #[error("indefinite-length items are forbidden")]
    IndefiniteLengthForbidden,

    /// Invalid UTF-8 string.
    #[error("invalid UTF-8 string")]
    InvalidUtf8,

    /// Unexpected end of input.
    #[error("unexpected end of input")]
    UnexpectedEof,

    /// Invalid CBOR structure.
    #[error("invalid CBOR structure: {message}")]
    InvalidStructure {
        /// Description of the structural error.
        message: String,
    },

    /// Unsupported CBOR type.
    #[error("unsupported CBOR type: {type_name}")]
    UnsupportedType {
        /// Name of the unsupported type.
        type_name: String,
    },

    /// Integer overflow during decoding.
    #[error("integer overflow")]
    IntegerOverflow,
}

impl CodecError {
    /// Create an encoding failed error.
    pub fn encoding_failed(message: impl Into<String>) -> Self {
        Self::EncodingFailed {
            message: message.into(),
        }
    }

    /// Create a decoding failed error.
    pub fn decoding_failed(message: impl Into<String>) -> Self {
        Self::DecodingFailed {
            message: message.into(),
        }
    }

    /// Create an invalid structure error.
    pub fn invalid_structure(message: impl Into<String>) -> Self {
        Self::InvalidStructure {
            message: message.into(),
        }
    }

    /// Create an unsupported type error.
    pub fn unsupported_type(type_name: impl Into<String>) -> Self {
        Self::UnsupportedType {
            type_name: type_name.into(),
        }
    }
}
