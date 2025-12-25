# Frameworks go here after cross-compilation:
#
# ios/Frameworks/
# └── entidb_ffi.xcframework/
#     ├── ios-arm64/
#     │   └── libentidb_ffi.a
#     ├── ios-arm64_x86_64-simulator/
#     │   └── libentidb_ffi.a
#     └── Info.plist
#
# Build commands:
#   cargo build --release --target aarch64-apple-ios -p entidb_ffi
#   cargo build --release --target aarch64-apple-ios-sim -p entidb_ffi
#   cargo build --release --target x86_64-apple-ios -p entidb_ffi
#
# Create xcframework:
#   xcodebuild -create-xcframework \
#     -library target/aarch64-apple-ios/release/libentidb_ffi.a \
#     -library target/aarch64-apple-ios-sim/release/libentidb_ffi.a \
#     -output ios/Frameworks/entidb_ffi.xcframework
