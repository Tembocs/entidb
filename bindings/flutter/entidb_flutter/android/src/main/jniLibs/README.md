# Native libraries go here after cross-compilation:
#
# android/src/main/jniLibs/
# ├── arm64-v8a/
# │   └── libentidb_ffi.so
# ├── armeabi-v7a/
# │   └── libentidb_ffi.so
# └── x86_64/
#     └── libentidb_ffi.so
#
# Build commands:
#   cargo ndk -t arm64-v8a -o ./jniLibs build --release -p entidb_ffi
#   cargo ndk -t armeabi-v7a -o ./jniLibs build --release -p entidb_ffi
#   cargo ndk -t x86_64 -o ./jniLibs build --release -p entidb_ffi
