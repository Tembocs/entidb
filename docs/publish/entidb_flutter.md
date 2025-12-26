# Publishing entidb_flutter to pub.dev

This document outlines the steps to build native libraries and publish the `entidb_flutter` Flutter plugin to pub.dev.

## Prerequisites

- Rust toolchain installed
- Android Studio with Android NDK installed
- Flutter SDK installed
- pub.dev account with publishing rights

## Environment Variables

Ensure the following are set:

```powershell
$env:ANDROID_NDK_HOME = "C:\Users\Tembo\AppData\Local\Android\Sdk\ndk\29.0.14206865"
```

---

## Step 1: Install cargo-ndk

```powershell
cargo install cargo-ndk
```

## Step 2: Add Android Rust Targets

```powershell
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
```

## Step 3: Build Android Native Libraries

```powershell
cd d:\rust\entidb
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 -o bindings/flutter/entidb_flutter/android/src/main/jniLibs build --release -p entidb_ffi
```

This creates:
```
android/src/main/jniLibs/
├── arm64-v8a/
│   └── libentidb_ffi.so
├── armeabi-v7a/
│   └── libentidb_ffi.so
└── x86_64/
    └── libentidb_ffi.so
```

## Step 4: Build Windows Native Library

```powershell
cargo build --release -p entidb_ffi
copy target\release\entidb_ffi.dll bindings\flutter\entidb_flutter\windows\libs\
```

## Step 5: Verify Native Libraries

```powershell
# Android
Get-ChildItem -Recurse bindings\flutter\entidb_flutter\android\src\main\jniLibs\*.so

# Windows
Get-ChildItem bindings\flutter\entidb_flutter\windows\libs\*.dll
```

## Step 6: Run Dry-Run Publish

```powershell
cd bindings\flutter\entidb_flutter
flutter pub publish --dry-run
```

Review the output for any warnings or errors.

## Step 7: Publish to pub.dev

```powershell
flutter pub publish
```

---

## Platform-Specific Notes

### Android
- Minimum SDK: 21 (Android 5.0)
- Supported ABIs: arm64-v8a, armeabi-v7a, x86_64
- Libraries placed in `android/src/main/jniLibs/<abi>/`

### Windows
- Target: x86_64-pc-windows-msvc
- Library: `entidb_ffi.dll`
- Placed in `windows/libs/`

### Linux
- Target: x86_64-unknown-linux-gnu
- Library: `libentidb_ffi.so`
- Placed in `linux/libs/`
- Build using WSL on Windows:
  ```bash
  wsl -e bash -c "source ~/.cargo/env && cd /mnt/d/rust/entidb && cargo build --release -p entidb_ffi"
  wsl -e bash -c "cp /mnt/d/rust/entidb/target/release/libentidb_ffi.so /mnt/d/rust/entidb/bindings/flutter/entidb_flutter/linux/libs/"
  ```

### macOS & iOS (via GitHub Actions)

Apple libraries require a macOS build environment. Use the GitHub Actions workflow:

1. **Trigger the workflow:**
   - Go to: `Actions` → `Build Apple Native Libraries`
   - Click `Run workflow` (or push a version tag like `v2.0.0-alpha.2`)

2. **Download artifacts:**
   - After workflow completes, download from the workflow run:
     - `macos-libentidb_ffi` → Universal dylib (arm64 + x86_64)
     - `ios-entidb_ffi-xcframework` → XCFramework for iOS

3. **Copy to Flutter plugin:**
   ```powershell
   # macOS
   copy libentidb_ffi.dylib bindings\flutter\entidb_flutter\macos\Libraries\
   
   # iOS (extract XCFramework)
   copy entidb_ffi.xcframework bindings\flutter\entidb_flutter\ios\Frameworks\ -Recurse
   ```

### macOS
- Targets: aarch64-apple-darwin, x86_64-apple-darwin (universal binary)
- Library: `libentidb_ffi.dylib`
- Placed in `macos/Libraries/`

### iOS
- Targets: aarch64-apple-ios (device), aarch64-apple-ios-sim + x86_64-apple-ios (simulators)
- Format: XCFramework
- Placed in `ios/Frameworks/`

---

## Troubleshooting

### cargo-ndk not finding NDK

Ensure `ANDROID_NDK_HOME` is set correctly:

```powershell
$env:ANDROID_NDK_HOME = "C:\Users\Tembo\AppData\Local\Android\Sdk\ndk\29.0.14206865"
```

### Missing Rust targets

```powershell
rustup target list --installed
```

Add missing targets with `rustup target add <target>`.

### Build failures

Check that `entidb_ffi` builds successfully in isolation:

```powershell
cargo build --release -p entidb_ffi
```

---

## Version Checklist

Before publishing, verify versions are synchronized:

- [ ] `pubspec.yaml` version matches intended release
- [ ] `CHANGELOG.md` is updated
- [ ] `entidb_dart` dependency version is correct
- [ ] All podspec versions match pubspec.yaml
