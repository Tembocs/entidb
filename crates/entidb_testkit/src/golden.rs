//! Golden test utilities for format verification.
//!
//! Provides helpers for verifying that encoded data matches
//! expected golden files.

use std::fs;
use std::path::{Path, PathBuf};

/// A golden test that compares output against expected files.
pub struct GoldenTest {
    name: String,
    golden_dir: PathBuf,
    update_mode: bool,
}

impl GoldenTest {
    /// Creates a new golden test.
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the test (used for file naming)
    /// * `golden_dir` - Directory containing golden files
    pub fn new(name: impl Into<String>, golden_dir: impl AsRef<Path>) -> Self {
        Self {
            name: name.into(),
            golden_dir: golden_dir.as_ref().to_path_buf(),
            update_mode: std::env::var("UPDATE_GOLDEN").is_ok(),
        }
    }

    /// Creates a golden test using the default test vectors directory.
    pub fn with_default_dir(name: impl Into<String>) -> Self {
        // Look for test_vectors in the workspace root
        let golden_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("docs").join("test_vectors"))
            .unwrap_or_else(|| PathBuf::from("test_vectors"));

        Self::new(name, golden_dir)
    }

    /// Asserts that the given bytes match the golden file.
    ///
    /// If `UPDATE_GOLDEN` environment variable is set, updates the golden file instead.
    pub fn assert_bytes(&self, suffix: &str, actual: &[u8]) {
        let path = self.file_path(suffix);

        if self.update_mode {
            self.update_golden_file(&path, actual);
            return;
        }

        if !path.exists() {
            panic!(
                "Golden file not found: {:?}\n\
                 Run with UPDATE_GOLDEN=1 to create it.\n\
                 Actual bytes (hex): {}",
                path,
                hex_encode(actual)
            );
        }

        let expected = fs::read(&path).expect("Failed to read golden file");

        if actual != expected {
            panic!(
                "Golden test '{}' failed for '{}':\n\
                 Expected ({} bytes): {}\n\
                 Actual ({} bytes): {}\n\
                 Run with UPDATE_GOLDEN=1 to update.",
                self.name,
                suffix,
                expected.len(),
                hex_encode(&expected),
                actual.len(),
                hex_encode(actual)
            );
        }
    }

    /// Asserts that the given string matches the golden file.
    pub fn assert_text(&self, suffix: &str, actual: &str) {
        let path = self.file_path(suffix);

        if self.update_mode {
            self.update_golden_file(&path, actual.as_bytes());
            return;
        }

        if !path.exists() {
            panic!(
                "Golden file not found: {:?}\n\
                 Run with UPDATE_GOLDEN=1 to create it.\n\
                 Actual:\n{}",
                path, actual
            );
        }

        let expected = fs::read_to_string(&path).expect("Failed to read golden file");

        if actual != expected {
            panic!(
                "Golden test '{}' failed for '{}':\n\
                 --- Expected ---\n{}\n\
                 --- Actual ---\n{}\n\
                 Run with UPDATE_GOLDEN=1 to update.",
                self.name, suffix, expected, actual
            );
        }
    }

    fn file_path(&self, suffix: &str) -> PathBuf {
        let filename = if suffix.is_empty() {
            format!("{}.golden", self.name)
        } else {
            format!("{}_{}.golden", self.name, suffix)
        };
        self.golden_dir.join(filename)
    }

    fn update_golden_file(&self, path: &Path, data: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("Failed to create golden directory");
        }
        fs::write(path, data).expect("Failed to write golden file");
        println!("Updated golden file: {:?}", path);
    }
}

/// Encodes bytes as hexadecimal string.
pub fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Decodes hexadecimal string to bytes.
pub fn hex_decode(hex: &str) -> Vec<u8> {
    let hex = hex.replace([' ', '\n', '\r'], "");
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("Invalid hex"))
        .collect()
}

/// CBOR test vectors for cross-language compatibility.
pub mod cbor_vectors {
    /// Standard CBOR test values.
    #[derive(Debug, Clone)]
    pub struct CborTestVector {
        /// Description of the test case
        pub description: &'static str,
        /// Expected CBOR bytes (hex-encoded)
        pub expected_hex: &'static str,
    }

    /// Returns the standard CBOR test vectors.
    #[must_use]
    pub fn standard_vectors() -> Vec<CborTestVector> {
        vec![
            CborTestVector {
                description: "Integer 0",
                expected_hex: "00",
            },
            CborTestVector {
                description: "Integer 1",
                expected_hex: "01",
            },
            CborTestVector {
                description: "Integer 23",
                expected_hex: "17",
            },
            CborTestVector {
                description: "Integer 24",
                expected_hex: "1818",
            },
            CborTestVector {
                description: "Integer 255",
                expected_hex: "18ff",
            },
            CborTestVector {
                description: "Integer 256",
                expected_hex: "190100",
            },
            CborTestVector {
                description: "Integer -1",
                expected_hex: "20",
            },
            CborTestVector {
                description: "Integer -24",
                expected_hex: "37",
            },
            CborTestVector {
                description: "Empty string",
                expected_hex: "60",
            },
            CborTestVector {
                description: "String 'a'",
                expected_hex: "6161",
            },
            CborTestVector {
                description: "Empty array",
                expected_hex: "80",
            },
            CborTestVector {
                description: "Empty map",
                expected_hex: "a0",
            },
            CborTestVector {
                description: "False",
                expected_hex: "f4",
            },
            CborTestVector {
                description: "True",
                expected_hex: "f5",
            },
            CborTestVector {
                description: "Null",
                expected_hex: "f6",
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex_roundtrip() {
        let original = vec![0x00, 0x01, 0xff, 0xab, 0xcd];
        let encoded = hex_encode(&original);
        let decoded = hex_decode(&encoded);
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_cbor_vectors_valid() {
        for vector in cbor_vectors::standard_vectors() {
            let bytes = hex_decode(vector.expected_hex);
            assert!(!bytes.is_empty(), "Vector '{}' should not be empty", vector.description);
        }
    }
}
