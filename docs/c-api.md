# C and C++ API

`crates/cadkit-capi` exposes cadkit through a small, stable C ABI plus a header-only C++17
wrapper.

- `crates/cadkit-capi/include/cadkit.h`: the C header (hand-maintained)
- `crates/cadkit-capi/include/cadkit.hpp`: `cadkit::Document` RAII wrapper
- `crates/cadkit-capi/examples/c/info.c`, `examples/cpp/info.cpp`, `CMakeLists.txt`

## Prebuilt libraries

Each GitHub release has a `cadkit-capi-<target>` archive (Linux x86_64 and aarch64 built
against glibc 2.28, macOS arm64 and x86_64, Windows x64) with `include/`, the shared and
static libraries in `lib/`, and the license files. `CADKIT_LIB_DIR` below can point at its
`lib/` folder.

## Build

```sh
python scripts/buildlock.py cargo build -p cadkit-capi --release
```

Artifacts in `target/release/`: the shared library (`cadkit_capi.dll` with import library
`cadkit_capi.dll.lib`, `libcadkit_capi.so`, `libcadkit_capi.dylib`) and the static library
(`cadkit_capi.lib` / `libcadkit_capi.a`). The static library needs the platform libraries
that Rust's std requires (`cargo rustc -p cadkit-capi -- --print native-static-libs` lists
them); prefer the shared library.

Examples with CMake:

```sh
cmake -S crates/cadkit-capi -B build/capi -DCADKIT_LIB_DIR=$PWD/target/release
cmake --build build/capi
./build/capi/info_c plan.dxf plan.svg
```

Or directly:

```sh
gcc -Icrates/cadkit-capi/include crates/cadkit-capi/examples/c/info.c \
    -Ltarget/release -lcadkit_capi -o info_c
```

With MSVC, link `cadkit_capi.dll.lib` and ship `cadkit_capi.dll` next to the executable.
With MinGW, link the DLL directly.

## Conventions

- Every fallible function returns `cadkit_status`; results come back through out-pointers,
  which are set to `NULL` on failure.
- `cadkit_last_error_message()` returns the message of the last failed call on the calling
  thread. It is owned by the library, never `NULL`, and valid until the next cadkit call on
  that thread. Do not free it.
- Strings are UTF-8 and NUL-terminated.
- No panic crosses the boundary: an internal panic is returned as `CADKIT_PANIC`.
- Every pointer argument is null-checked; passing `NULL` returns `CADKIT_NULL_POINTER`.
- Not thread-affine: a document is immutable and may be read from several threads at once.
  Do not free it while another thread uses it.

## Ownership

| Object | Created by | Released by |
|---|---|---|
| `cadkit_document*` | `cadkit_read_file`, `cadkit_read_bytes` | `cadkit_document_free` (once; `NULL` is a no-op) |
| `char*` | `cadkit_document_to_json`, `_to_svg`, `_to_dxf`, `_info_json` | `cadkit_string_free` (once; `NULL` is a no-op) |
| `const char*` | `cadkit_version`, `cadkit_status_string`, `cadkit_last_error_message` | never freed |

Freeing a document twice, or using one after it was freed, is detected through a registry of
live handles and returns `CADKIT_INVALID_HANDLE`. This is a safety net, not a contract: if
the freed address is reused by a later document the stale pointer cannot be told apart.

## Functions

| Function | Purpose |
|---|---|
| `cadkit_version` | Library version string |
| `cadkit_status_string(status)` | Static text for a status code |
| `cadkit_last_error_message` | Thread-local message of the last failure |
| `cadkit_options_init`, `cadkit_svg_options_init` | Fill option structs with defaults |
| `cadkit_detect(data, len)` | Format from the header only |
| `cadkit_read_file(path, options, &doc)` | Read a drawing from a UTF-8 path |
| `cadkit_read_bytes(data, len, options, &doc)` | Read a drawing from memory |
| `cadkit_document_free(doc)` | Release a document |
| `cadkit_document_info(doc, &info)` | Format and counts as a plain struct |
| `cadkit_document_info_json(doc, &s)` | Summary as JSON (adds version, application, per-kind counts) |
| `cadkit_document_to_json(doc, pretty, &s)` | Full model as JSON |
| `cadkit_document_to_svg(doc, options, &s)` | One model rendered as SVG |
| `cadkit_document_to_dxf(doc, version, &s)` | ASCII DXF of the given version |
| `cadkit_string_free(s)` | Release a returned string |

### Status codes

| Code | Value | Meaning |
|---|---|---|
| `CADKIT_OK` | 0 | Success |
| `CADKIT_NULL_POINTER` | 1 | A required pointer was `NULL` |
| `CADKIT_INVALID_ARGUMENT` | 2 | Out-of-range value or invalid UTF-8 |
| `CADKIT_IO` | 3 | File could not be read |
| `CADKIT_TRUNCATED` | 4 | Input ended early |
| `CADKIT_INVALID_DATA` | 5 | Input violates the format |
| `CADKIT_UNSUPPORTED` | 6 | Valid input using a feature that is not implemented |
| `CADKIT_LIMIT_EXCEEDED` | 7 | A configured limit was exceeded |
| `CADKIT_UNKNOWN_FORMAT` | 8 | Not a recognized drawing format |
| `CADKIT_PANIC` | 9 | Internal panic caught (a bug; please report) |
| `CADKIT_INTERNAL` | 10 | Unexpected internal failure |
| `CADKIT_INVALID_HANDLE` | 11 | Handle is not a live document |

Enum values are part of the ABI and never change; new values may be added.

### Options

`cadkit_options`: resource limits (`max_input_bytes`, `max_decompressed_bytes`,
`max_objects`, `max_depth`, `max_string_bytes`, `max_vertices`; zero selects the default),
`keep_raw`, `fallback_codepage` (WHATWG label such as `"windows-1254"`). Pass `NULL` for all
defaults. Lower the limits when reading untrusted files (see [SECURITY.md](../SECURITY.md)).

`cadkit_svg_options`: `model_index`, `width_px`, `stroke_px` (both must be finite and > 0),
`background` (CSS color, `NULL` for transparent), `text`, `expand_blocks`. Pass `NULL` for
defaults, or start from `cadkit_svg_options_init`.

## Example (C)

```c
#include <stdio.h>
#include "cadkit.h"

int main(void) {
    cadkit_document *doc = NULL;
    cadkit_status st = cadkit_read_file("plan.dxf", NULL, &doc);
    if (st != CADKIT_OK) {
        fprintf(stderr, "%s: %s\n", cadkit_status_string(st), cadkit_last_error_message());
        return 1;
    }
    cadkit_info info;
    if (cadkit_document_info(doc, &info) == CADKIT_OK)
        printf("%llu entities\n", (unsigned long long)info.entity_count);

    char *svg = NULL;
    if (cadkit_document_to_svg(doc, NULL, &svg) == CADKIT_OK) {
        fputs(svg, stdout);
        cadkit_string_free(svg);
    }
    cadkit_document_free(doc);
    return 0;
}
```

## C++ wrapper

`cadkit::Document` is move-only and releases its handle in the destructor. Failures throw
`cadkit::Error` (derives `std::runtime_error`; `status()` returns the `cadkit_status`, `what()`
the library message). Getters return `std::string`.

```cpp
#include <iostream>
#include "cadkit.hpp"

int main() {
    try {
        auto doc = cadkit::Document::read_file("plan.dxf");
        std::cout << doc.info().entity_count << " entities\n";
        cadkit::SvgOptions svg;
        svg.width_px = 1200;
        std::string text = doc.to_svg(svg);
        std::string dxf = doc.to_dxf(CADKIT_DXF_R2000);
    } catch (const cadkit::Error& e) {
        std::cerr << e.what() << " (status " << static_cast<int>(e.status()) << ")\n";
        return 1;
    }
}
```

## Stability

The header is the contract. Functions are only added in minor versions; existing signatures,
enum values and struct layouts change only with a major version (structs gain fields at the
end only after a major bump, which is why options are filled with the `_init` functions).
Files that use features a reader does not handle return `CADKIT_UNSUPPORTED`; the support
matrix is in the [README](../README.md).

## DGN V8 and native CityGML

`CADKIT_FORMAT_CITY_GML = 5` extends the format enum without changing existing
values or struct layouts. New entry points are declared in `cadkit.h`:

- `cadkit_document_from_json` creates a neutral document.
- `cadkit_document_to_dgn` accepts seed bytes and optional Rust `WriteOptions`
  JSON, returning a pointer and byte length. Release with `cadkit_bytes_free`;
  never pass the binary buffer to `cadkit_string_free`.
- `cadkit_document_to_citygml` accepts an explicit CRS and LoD, returning a UTF-8
  string to release with `cadkit_string_free`.
- `cadkit_citygml_read_json` / `cadkit_citygml_write_json` expose the native
  CityGML tree without losing semantic relationships through neutral geometry.
  Write options follow Rust `gml::ValidationOptions` JSON.

Null/error paths reset outputs. Owned binary buffers are tracked to make repeated
free calls harmless. As with existing document handles, caller-controlled input
pointers must remain valid for the call and live documents cannot be freed
concurrently. C++ adds RAII `Document::from_json`, `to_dgn` and `to_citygml`.
