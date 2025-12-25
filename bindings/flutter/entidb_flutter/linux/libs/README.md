# Native libraries go here after cross-compilation:
#
# linux/libs/
# └── libentidb_ffi.so
#
# Build command:
#   cargo build --release --target x86_64-unknown-linux-gnu -p entidb_ffi
#
# Copy:
#   cp target/x86_64-unknown-linux-gnu/release/libentidb_ffi.so linux/libs/
