# cadkit-capi

Stable C ABI and C++ header for [cadkit](https://github.com/TNYCL/cadkit): read DWG, DGN,
DXF and CityGML 2.0 drawings and write DXF, DGN V8, CityGML 2.0, SVG and JSON from C or C++.

The crate builds `cadkit_capi` as a shared library (`cdylib`), a static library
(`staticlib`) and an `rlib`. The headers are `include/cadkit.h` (C99) and
`include/cadkit.hpp` (C++17); `cadkit.h` documents status codes, ownership and threading.

## Prebuilt libraries

Most C and C++ users want the prebuilt archives on
[GitHub Releases](https://github.com/TNYCL/cadkit/releases/latest)
(`cadkit-capi-<target>`, with headers, shared and static libraries) for Linux (glibc 2.28+),
macOS and Windows x64.

## Building from source

```sh
cargo build -p cadkit-capi --release
```

The libraries land in `target/release`. The C and C++ examples and the CMake files live in
the [repository](https://github.com/TNYCL/cadkit/tree/main/crates/cadkit-capi) and build
against a workspace checkout.

## License

MIT OR Apache-2.0.
