# Native libraries go here after cross-compilation:
#
# macos/Libraries/
# └── libentidb_ffi.dylib (universal binary: arm64 + x86_64)
#
# Build commands:
#   cargo build --release --target aarch64-apple-darwin -p entidb_ffi
#   cargo build --release --target x86_64-apple-darwin -p entidb_ffi
#
# Create universal binary:
#   lipo -create \
#     target/aarch64-apple-darwin/release/libentidb_ffi.dylib \
#     target/x86_64-apple-darwin/release/libentidb_ffi.dylib \
#     -output macos/Libraries/libentidb_ffi.dylib
