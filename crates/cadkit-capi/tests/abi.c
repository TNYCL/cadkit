/* Compile-time ABI check of include/cadkit.h; mirrors tests/abi.rs.
 *   cc -std=c11 -fsyntax-only -Iinclude tests/abi.c      (g++/clang++ also accepted) */
#include <stddef.h>
#include <stdint.h>

#include "cadkit.h"

#ifdef __cplusplus
#define ASSERT(cond) static_assert(cond, #cond)
#else
#define ASSERT(cond) _Static_assert(cond, #cond)
#endif

ASSERT(sizeof(cadkit_status) == 4);
ASSERT(sizeof(cadkit_format) == 4);
ASSERT(sizeof(cadkit_dxf_version) == 4);

ASSERT(CADKIT_OK == 0);
ASSERT(CADKIT_NULL_POINTER == 1);
ASSERT(CADKIT_INVALID_ARGUMENT == 2);
ASSERT(CADKIT_IO == 3);
ASSERT(CADKIT_TRUNCATED == 4);
ASSERT(CADKIT_INVALID_DATA == 5);
ASSERT(CADKIT_UNSUPPORTED == 6);
ASSERT(CADKIT_LIMIT_EXCEEDED == 7);
ASSERT(CADKIT_UNKNOWN_FORMAT == 8);
ASSERT(CADKIT_PANIC == 9);
ASSERT(CADKIT_INTERNAL == 10);
ASSERT(CADKIT_INVALID_HANDLE == 11);

ASSERT(CADKIT_FORMAT_UNKNOWN == 0);
ASSERT(CADKIT_FORMAT_DWG == 1);
ASSERT(CADKIT_FORMAT_DXF == 2);
ASSERT(CADKIT_FORMAT_DGN_V7 == 3);
ASSERT(CADKIT_FORMAT_DGN_V8 == 4);

ASSERT(CADKIT_DXF_R12 == 0);
ASSERT(CADKIT_DXF_R2000 == 1);
ASSERT(CADKIT_DXF_R2004 == 2);
ASSERT(CADKIT_DXF_R2007 == 3);
ASSERT(CADKIT_DXF_R2010 == 4);
ASSERT(CADKIT_DXF_R2013 == 5);
ASSERT(CADKIT_DXF_R2018 == 6);

#if UINTPTR_MAX == 0xFFFFFFFFFFFFFFFFu
ASSERT(sizeof(cadkit_options) == 48);
ASSERT(offsetof(cadkit_options, max_input_bytes) == 0);
ASSERT(offsetof(cadkit_options, max_decompressed_bytes) == 8);
ASSERT(offsetof(cadkit_options, max_objects) == 16);
ASSERT(offsetof(cadkit_options, max_depth) == 24);
ASSERT(offsetof(cadkit_options, max_string_bytes) == 28);
ASSERT(offsetof(cadkit_options, max_vertices) == 32);
ASSERT(offsetof(cadkit_options, keep_raw) == 36);
ASSERT(offsetof(cadkit_options, fallback_codepage) == 40);

ASSERT(sizeof(cadkit_svg_options) == 40);
ASSERT(offsetof(cadkit_svg_options, model_index) == 0);
ASSERT(offsetof(cadkit_svg_options, width_px) == 8);
ASSERT(offsetof(cadkit_svg_options, stroke_px) == 16);
ASSERT(offsetof(cadkit_svg_options, background) == 24);
ASSERT(offsetof(cadkit_svg_options, text) == 32);
ASSERT(offsetof(cadkit_svg_options, expand_blocks) == 36);

ASSERT(sizeof(cadkit_info) == 48);
ASSERT(offsetof(cadkit_info, format) == 0);
ASSERT(offsetof(cadkit_info, model_count) == 8);
ASSERT(offsetof(cadkit_info, layer_count) == 16);
ASSERT(offsetof(cadkit_info, block_count) == 24);
ASSERT(offsetof(cadkit_info, entity_count) == 32);
ASSERT(offsetof(cadkit_info, warning_count) == 40);
#endif

int main(void) { return 0; }
