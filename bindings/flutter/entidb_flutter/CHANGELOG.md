# Changelog

## 2.0.0-alpha.2

Full platform support release.

### Added

- ✅ Android native libraries (arm64-v8a, armeabi-v7a, x86_64)
- ✅ iOS XCFramework (device + simulator)
- ✅ macOS universal binary (arm64 + x86_64)
- ✅ Linux native library (x86_64)
- GitHub Actions workflow for building Apple libraries

### Changed

- All platforms now have bundled native libraries
- Package size increased to ~7 MB (compressed) to include all binaries

---

## 2.0.0-alpha.1

Initial release of `entidb_flutter` - Flutter plugin for EntiDB.

### Features

- **Flutter FFI Plugin**: Uses `ffiPlugin: true` for automatic native library bundling
- **Platform Support**: Android, iOS, macOS, Windows, Linux scaffold ready
- **Re-exports entidb_dart**: All database APIs available through a single import

### Platform Status

- ✅ Windows - Native library ready
- 🚧 Android - Plugin scaffold ready, native libraries pending
- 🚧 iOS - Plugin scaffold ready, native libraries pending  
- 🚧 macOS - Plugin scaffold ready, native libraries pending
- 🚧 Linux - Plugin scaffold ready, native libraries pending

### Notes

- This is an alpha release. Native libraries for mobile platforms are still being cross-compiled.
- Windows users can use the plugin immediately.
- For other platforms, use `entidb_dart` directly with your own native library builds.
