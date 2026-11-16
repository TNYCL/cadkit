cmake_minimum_required(VERSION 3.18)
project(quickstart LANGUAGES C)

# CADKIT_DIR: an unpacked cadkit-capi-<target> archive from GitHub Releases (include/ and lib/).
set(CADKIT_DIR "" CACHE PATH "Unpacked cadkit-capi archive")
# On Windows, name the import library: NAMES cadkit_capi.dll finds cadkit_capi.dll.lib.
find_library(CADKIT_LIBRARY NAMES cadkit_capi PATHS "${CADKIT_DIR}/lib" NO_DEFAULT_PATH REQUIRED)

add_executable(quickstart quickstart.c)
target_include_directories(quickstart PRIVATE "${CADKIT_DIR}/include")
target_link_libraries(quickstart PRIVATE "${CADKIT_LIBRARY}")
