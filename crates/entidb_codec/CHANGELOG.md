# Changelog

All notable changes to the `entidb_codec` crate will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2024-XX-XX

### Added

- Canonical CBOR encoder following EntiDB deterministic encoding rules:
  - Maps sorted by key (bytewise)
  - Integers use shortest encoding
  - No indefinite-length items
  - UTF-8 string validation
- Canonical CBOR decoder with strict validation
- `Value` enum for representing CBOR values
- `Encoder` struct for streaming encoding
- `Decoder` struct for streaming decoding
- Test vectors for cross-language parity verification
- Error types for encoding/decoding failures
