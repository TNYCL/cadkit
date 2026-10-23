# Minimal build of the C and C++ examples against a built cadkit-capi library.
#
#   python scripts/buildlock.py cargo build -p cadkit-capi --release
#   cmake -S crates/cadkit-capi -B build/capi -DCADKIT_LIB_DIR=<repo>/target/release
#   cmake --build build/capi
cmake_minimum_required(VERSION 3.16)
file(STRINGS "${CMAKE_CURRENT_SOURCE_DIR}/../../Cargo.toml" CADKIT_VERSION_LINE REGEX "^version = ")
string(REGEX MATCH "[0-9]+\\.[0-9]+\\.[0-9]+" CADKIT_VERSION "${CADKIT_VERSION_LINE}")
project(cadkit_capi_examples VERSION ${CADKIT_VERSION} LANGUAGES C CXX)
include(GNUInstallDirs)
include(CMakePackageConfigHelpers)

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


# Install the Rust-built shared library without rebuilding it in CMake.
install(FILES "${CADKIT_LIBRARY}" DESTINATION "${CMAKE_INSTALL_LIBDIR}")
if(WIN32 AND MSVC)
  install(FILES "${CADKIT_DLL}" DESTINATION "${CMAKE_INSTALL_BINDIR}")
endif()
install(FILES include/cadkit.h include/cadkit.hpp DESTINATION "${CMAKE_INSTALL_INCLUDEDIR}")
get_filename_component(CADKIT_LIBRARY_NAME "${CADKIT_LIBRARY}" NAME)
configure_package_config_file(cmake/cadkitConfig.cmake.in
  "${CMAKE_CURRENT_BINARY_DIR}/cadkitConfig.cmake"
  INSTALL_DESTINATION "${CMAKE_INSTALL_LIBDIR}/cmake/cadkit")
write_basic_package_version_file("${CMAKE_CURRENT_BINARY_DIR}/cadkitConfigVersion.cmake"
  VERSION "${PROJECT_VERSION}" COMPATIBILITY SameMinorVersion)
install(FILES "${CMAKE_CURRENT_BINARY_DIR}/cadkitConfig.cmake"
  "${CMAKE_CURRENT_BINARY_DIR}/cadkitConfigVersion.cmake"
  DESTINATION "${CMAKE_INSTALL_LIBDIR}/cmake/cadkit")
file(RELATIVE_PATH CADKIT_PC_PREFIX "${CMAKE_INSTALL_PREFIX}/${CMAKE_INSTALL_LIBDIR}/pkgconfig" "${CMAKE_INSTALL_PREFIX}")
configure_file(cadkit.pc.in "${CMAKE_CURRENT_BINARY_DIR}/cadkit.pc" @ONLY)
install(FILES "${CMAKE_CURRENT_BINARY_DIR}/cadkit.pc" DESTINATION "${CMAKE_INSTALL_LIBDIR}/pkgconfig")

include(CTest)
if(BUILD_TESTING)
  add_executable(contract_cpp tests/contract.cpp)
  target_compile_features(contract_cpp PRIVATE cxx_std_17)
  target_link_libraries(contract_cpp PRIVATE cadkit)
  add_test(NAME cpp_contract COMMAND contract_cpp)
  if(CADKIT_DLL)
    add_custom_command(TARGET contract_cpp POST_BUILD
      COMMAND ${CMAKE_COMMAND} -E copy_if_different "${CADKIT_DLL}" "$<TARGET_FILE_DIR:contract_cpp>")
  endif()
endif()
