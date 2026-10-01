# Minimal build of the C and C++ examples against a built cadkit-capi library.
#
#   python scripts/buildlock.py cargo build -p cadkit-capi --release
#   cmake -S crates/cadkit-capi -B build/capi -DCADKIT_LIB_DIR=<repo>/target/release
#   cmake --build build/capi
cmake_minimum_required(VERSION 3.16)
project(cadkit_capi_examples LANGUAGES C CXX)

set(CADKIT_LIB_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../../target/release" CACHE PATH "Directory with the cadkit_capi library")

if(WIN32)
  find_file(CADKIT_DLL NAMES cadkit_capi.dll PATHS "${CADKIT_LIB_DIR}" NO_DEFAULT_PATH)
  if(MSVC)
    find_file(CADKIT_LIBRARY NAMES cadkit_capi.dll.lib PATHS "${CADKIT_LIB_DIR}" NO_DEFAULT_PATH)
  else()
    set(CADKIT_LIBRARY "${CADKIT_DLL}") # MinGW links the DLL directly
  endif()
else()
  find_library(CADKIT_LIBRARY NAMES cadkit_capi PATHS "${CADKIT_LIB_DIR}" NO_DEFAULT_PATH)
endif()
if(NOT CADKIT_LIBRARY)
  message(FATAL_ERROR "cadkit_capi not found in ${CADKIT_LIB_DIR}; build it with cargo build -p cadkit-capi")
endif()

add_library(cadkit INTERFACE)
target_include_directories(cadkit INTERFACE "${CMAKE_CURRENT_SOURCE_DIR}/include")
target_link_libraries(cadkit INTERFACE "${CADKIT_LIBRARY}")

add_executable(info_c examples/c/info.c)
target_link_libraries(info_c PRIVATE cadkit)

add_executable(info_cpp examples/cpp/info.cpp)
target_compile_features(info_cpp PRIVATE cxx_std_17)
target_link_libraries(info_cpp PRIVATE cadkit)

if(CADKIT_DLL)
  foreach(t info_c info_cpp)
    add_custom_command(TARGET ${t} POST_BUILD
      COMMAND ${CMAKE_COMMAND} -E copy_if_different "${CADKIT_DLL}" "$<TARGET_FILE_DIR:${t}>")
  endforeach()
endif()
