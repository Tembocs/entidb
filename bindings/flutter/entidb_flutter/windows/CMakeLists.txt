# The Flutter tooling requires that developers have CMake 3.10 or later
# installed. You should not increase this version, as doing so will cause
# the plugin to fail to compile for some customers of the plugin.
cmake_minimum_required(VERSION 3.10)

project(entidb_flutter_library VERSION 2.0.0 LANGUAGES C)

# Locate the prebuilt DLL
set(ENTIDB_DLL "${CMAKE_CURRENT_SOURCE_DIR}/libs/entidb_ffi.dll")

# This will be bundled with the app
set(entidb_flutter_bundled_libraries
  "${ENTIDB_DLL}"
  PARENT_SCOPE
)
