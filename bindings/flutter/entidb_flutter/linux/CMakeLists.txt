# The Flutter tooling requires that developers have CMake 3.10 or later
# installed. You should not increase this version, as doing so will cause
# the plugin to fail to compile for some customers of the plugin.
cmake_minimum_required(VERSION 3.10)

project(entidb_flutter_library VERSION 2.0.0 LANGUAGES C)

# Locate the prebuilt shared library
set(ENTIDB_SO "${CMAKE_CURRENT_SOURCE_DIR}/libs/libentidb_ffi.so")

# This will be bundled with the app
set(entidb_flutter_bundled_libraries
  "${ENTIDB_SO}"
  PARENT_SCOPE
)
