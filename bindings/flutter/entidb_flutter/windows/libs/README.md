# Native libraries go here after cross-compilation:
#
# windows/libs/
# └── entidb_ffi.dll
#
# Build command:
#   cargo build --release --target x86_64-pc-windows-msvc -p entidb_ffi
#
# Copy:
#   copy target\x86_64-pc-windows-msvc\release\entidb_ffi.dll windows\libs\
