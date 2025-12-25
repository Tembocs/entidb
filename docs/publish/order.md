# EntiDB Publication Order

This document defines the required order for publishing EntiDB packages across all platforms.

---

## Overview

EntiDB consists of multiple packages published to different registries:

| Registry | Packages |
|----------|----------|
| crates.io | 9 Rust crates |
| pub.dev | 1 Dart package |
| PyPI | 1 Python package |
| npm | 1 WASM package |

---

## Publication Order

### Phase 1: Base Rust Crates (No Internal Dependencies)

These crates have no dependencies on other EntiDB crates and must be published first:

| Order | Crate | Command |
|-------|-------|---------|
| 1 | `entidb_storage` | `cargo publish -p entidb_storage` |
| 2 | `entidb_codec` | `cargo publish -p entidb_codec` |

### Phase 2: Core Rust Crates

These depend on the base crates:

| Order | Crate | Dependencies | Command |
|-------|-------|--------------|---------|
| 3 | `entidb_core` | storage, codec | `cargo publish -p entidb_core` |

### Phase 3: FFI and Protocol Crates

These depend on core:

| Order | Crate | Dependencies | Command |
|-------|-------|--------------|---------|
| 4 | `entidb_ffi` | core, codec, storage | `cargo publish -p entidb_ffi` |
| 5 | `entidb_sync_protocol` | codec | `cargo publish -p entidb_sync_protocol` |

### Phase 4: Higher-Level Rust Crates

| Order | Crate | Dependencies | Command |
|-------|-------|--------------|---------|
| 6 | `entidb_sync_engine` | core, sync_protocol | `cargo publish -p entidb_sync_engine` |
| 7 | `entidb_sync_server` | core, sync_engine, sync_protocol | `cargo publish -p entidb_sync_server` |
| 8 | `entidb_testkit` | core, codec, storage | `cargo publish -p entidb_testkit` |
| 9 | `entidb_cli` | core, codec, storage | `cargo publish -p entidb_cli` |

### Phase 5: Language Bindings

After all Rust crates are published:

| Order | Package | Registry | Command |
|-------|---------|----------|---------|
| 10 | `entidb_dart` | pub.dev | `cd bindings/dart/entidb_dart && dart pub publish` |
| 11 | `entidb` (Python) | PyPI | `cd bindings/python/entidb_py && maturin publish` |
| 12 | `entidb_wasm` | npm | `cd web/entidb_wasm && wasm-pack publish` |

---

## Pre-Publication Checklist

Before publishing each package, verify:

- [ ] All tests pass (`cargo test --all`)
- [ ] Version numbers are consistent across all packages
- [ ] CHANGELOG.md is updated
- [ ] README.md exists and is accurate
- [ ] License files are present
- [ ] `cargo publish --dry-run` succeeds (for Rust crates)
- [ ] `dart pub publish --dry-run` succeeds (for Dart)
- [ ] `maturin build --release` succeeds (for Python)
- [ ] `wasm-pack build --target web` succeeds (for WASM)

---

## Version Synchronization

All packages must maintain synchronized versions:

| Package | Current Version |
|---------|-----------------|
| All Rust crates | 0.1.0 |
| entidb_dart | 0.1.0 |
| entidb (Python) | 0.1.0 |
| entidb_wasm | 0.1.0 |

When bumping versions, update:
1. Workspace `Cargo.toml` (`version = "X.Y.Z"`)
2. All crate dependency versions in workspace
3. `bindings/dart/entidb_dart/pubspec.yaml`
4. `bindings/python/entidb_py/pyproject.toml`
5. `web/entidb_wasm/Cargo.toml`

---

## Dependency Graph

```
entidb_storage ─────────────────────────────────────┐
                                                    │
entidb_codec ──────────────────────────────────────┐│
                                                   ││
                    ┌──────────────────────────────┼┼─► entidb_testkit
                    │                              ││
                    ▼                              ││
              entidb_core ◄────────────────────────┴┴───► entidb_cli
                    │
                    ├──────────────────────────────────► entidb_ffi
                    │                                         │
                    ▼                                         ▼
         entidb_sync_protocol                           [Dart binding]
                    │                                   [Python binding]
                    ▼
          entidb_sync_engine
                    │
                    ▼
          entidb_sync_server
```

---

## Registry Credentials

Required credentials for publication:

| Registry | Credential | Setup Command |
|----------|------------|---------------|
| crates.io | API token | `cargo login` |
| pub.dev | Google account | `dart pub login` |
| PyPI | API token | Configure in `~/.pypirc` or `MATURIN_PYPI_TOKEN` |
| npm | Auth token | `npm login` |

---

## Rollback Procedure

If a publication fails mid-sequence:

1. **Do not** yank published crates unless they contain critical bugs
2. Fix the issue in the failing crate
3. Bump patch version if changes were needed
4. Continue from the failed step

---

## Notes

- Always publish from a clean git state (no uncommitted changes)
- Tag releases in git after successful publication: `git tag v0.1.0`
- Wait for crates.io index to update (~5-10 minutes) before publishing dependent crates
- The `--dry-run` flag cannot verify crates with unpublished dependencies
