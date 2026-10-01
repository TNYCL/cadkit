/*
 * cadkit C API
 *
 * Read DWG, DGN and DXF drawings into one format-neutral model, from C or C++.
 * Copyright (c) 2026 TNYCL (https://tnycl.com).
 * License: MIT OR Apache-2.0.
 *
 * Conventions
 *   - Every fallible function returns a cadkit_status. Results are delivered through
 *     out-pointers, which are set to NULL on failure.
 *   - After a failure, cadkit_last_error_message() describes it. The message is
 *     thread-local, owned by the library, and valid until the next cadkit call on the
 *     same thread. Do not free it.
 *   - Strings are UTF-8, NUL-terminated.
 *   - Ownership:
 *       cadkit_document*  created by cadkit_read_file / cadkit_read_bytes,
 *                         released with cadkit_document_free (exactly once).
 *       char*             returned by cadkit_document_to_* / cadkit_document_info_json,
 *                         released with cadkit_string_free (exactly once).
 *       const char*       returned by cadkit_version / cadkit_status_string: static, do not free.
 *   - A document is immutable: it may be read from several threads at once, but must not be
 *     freed while another thread is using it.
 *   - No panic crosses this boundary; an internal bug is reported as CADKIT_PANIC.
 *   - Freeing a handle twice, or using a freed handle, is detected and reported as
 *     CADKIT_INVALID_HANDLE (best effort; do not rely on it).
 *   - Structs are initialized with cadkit_options_init / cadkit_svg_options_init so that
 *     fields added in later versions get defaults.
 */
#ifndef CADKIT_H
#define CADKIT_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#if defined(_WIN32) && defined(CADKIT_SHARED_IMPORT)
#define CADKIT_API __declspec(dllimport)
#else
#define CADKIT_API
#endif

/* Result code of every fallible call. Values are stable. */
typedef enum cadkit_status {
    CADKIT_OK = 0,
    CADKIT_NULL_POINTER = 1,
    CADKIT_INVALID_ARGUMENT = 2,
    CADKIT_IO = 3,
    CADKIT_TRUNCATED = 4,
    CADKIT_INVALID_DATA = 5,
    CADKIT_UNSUPPORTED = 6,
    CADKIT_LIMIT_EXCEEDED = 7,
    CADKIT_UNKNOWN_FORMAT = 8,
    CADKIT_PANIC = 9,
    CADKIT_INTERNAL = 10,
    CADKIT_INVALID_HANDLE = 11
} cadkit_status;

/* File format. Values are stable. */
typedef enum cadkit_format {
    CADKIT_FORMAT_UNKNOWN = 0,
    CADKIT_FORMAT_DWG = 1,
    CADKIT_FORMAT_DXF = 2,
    CADKIT_FORMAT_DGN_V7 = 3,
    CADKIT_FORMAT_DGN_V8 = 4,
    CADKIT_FORMAT_CITY_GML = 5
} cadkit_format;

/* DXF output version. Values are stable. */
typedef enum cadkit_dxf_version {
    CADKIT_DXF_R12 = 0,   /* AC1009 */
    CADKIT_DXF_R2000 = 1, /* AC1015 */
    CADKIT_DXF_R2004 = 2, /* AC1018 */
    CADKIT_DXF_R2007 = 3, /* AC1021 */
    CADKIT_DXF_R2010 = 4, /* AC1024 */
    CADKIT_DXF_R2013 = 5, /* AC1027 */
    CADKIT_DXF_R2018 = 6  /* AC1032 */
} cadkit_dxf_version;

/* Opaque document handle. */
typedef struct cadkit_document cadkit_document;

/* Read options. A zero numeric field selects the library default; a NULL options pointer
 * selects all defaults. Always initialize with cadkit_options_init. */
typedef struct cadkit_options {
    uint64_t max_input_bytes;        /* largest accepted input (0 = default) */
    uint64_t max_decompressed_bytes; /* largest total decompressed size (0 = default) */
    uint64_t max_objects;            /* most records per document (0 = default) */
    uint32_t max_depth;              /* deepest nesting (0 = default) */
    uint32_t max_string_bytes;       /* longest single string (0 = default) */
    uint32_t max_vertices;           /* most vertices in one entity (0 = default) */
    int32_t keep_raw;                /* non-zero: keep original record bytes */
    const char *fallback_codepage;   /* e.g. "windows-1254"; may be NULL */
} cadkit_options;

/* SVG options. Initialize with cadkit_svg_options_init. */
typedef struct cadkit_svg_options {
    uint32_t model_index;   /* which model to render */
    double width_px;        /* output width in CSS pixels, > 0 */
    double stroke_px;       /* stroke width in CSS pixels, > 0 */
    const char *background; /* CSS color, or NULL for transparent */
    int32_t text;           /* non-zero: render text entities */
    int32_t expand_blocks;  /* non-zero: expand block references */
} cadkit_svg_options;

/* Document summary (plain data). */
typedef struct cadkit_info {
    cadkit_format format;
    uint64_t model_count;
    uint64_t layer_count;
    uint64_t block_count;
    uint64_t entity_count; /* top-level entities over all models */
    uint64_t warning_count;
} cadkit_info;

/* Library version, e.g. "0.1.0". Static. */
CADKIT_API const char *cadkit_version(void);

/* Static description of a cadkit_status value. */
CADKIT_API const char *cadkit_status_string(int32_t status);

/* Message of the last failed call on this thread; "" if none. Never NULL. */
CADKIT_API const char *cadkit_last_error_message(void);

CADKIT_API void cadkit_options_init(cadkit_options *options);
CADKIT_API void cadkit_svg_options_init(cadkit_svg_options *options);

/* Detects the format from the header only. CADKIT_FORMAT_UNKNOWN for NULL or unknown data. */
CADKIT_API cadkit_format cadkit_detect(const uint8_t *data, size_t len);

/* Reads a drawing. On success *out owns a document (free with cadkit_document_free);
 * on failure *out is NULL. `options` may be NULL. CADKIT_UNSUPPORTED is returned
 * for files that use features the readers do not handle. DWG and DGN are read, not written. */
CADKIT_API cadkit_status cadkit_read_file(const char *utf8_path, const cadkit_options *options,
                                          cadkit_document **out);
CADKIT_API cadkit_status cadkit_read_bytes(const uint8_t *data, size_t len,
                                           const cadkit_options *options, cadkit_document **out);

/* Releases a document. NULL is a no-op. */
CADKIT_API cadkit_status cadkit_document_free(cadkit_document *doc);

/* Summary as plain data. */
CADKIT_API cadkit_status cadkit_document_info(const cadkit_document *doc, cadkit_info *out);

/* Getters returning owned strings (free each with cadkit_string_free). *out is NULL on failure. */
CADKIT_API cadkit_status cadkit_document_info_json(const cadkit_document *doc, char **out);
CADKIT_API cadkit_status cadkit_document_to_json(const cadkit_document *doc, int pretty, char **out);
CADKIT_API cadkit_status cadkit_document_to_svg(const cadkit_document *doc,
                                                const cadkit_svg_options *options, char **out);
CADKIT_API cadkit_status cadkit_document_to_dxf(const cadkit_document *doc, cadkit_dxf_version version,
                                                char **out);

/* Yeni geometri ve CityGML modelleri; dönen dizeler cadkit_string_free ile bırakılır. */
CADKIT_API cadkit_status cadkit_document_from_json(const char *json, cadkit_document **out);
/* İkili çıktıyı yalnız cadkit_bytes_free ile bırakın; out_len bayt uzunluğudur. */
CADKIT_API cadkit_status cadkit_document_to_dgn(const cadkit_document *doc, const uint8_t *seed,
    size_t seed_len, const char *options_json, uint8_t **out, size_t *out_len);
CADKIT_API void cadkit_bytes_free(uint8_t *data);
CADKIT_API cadkit_status cadkit_document_to_citygml(const cadkit_document *doc,
    const char *srs_name, uint8_t lod, char **out);
CADKIT_API cadkit_status cadkit_citygml_read_json(const uint8_t *data, size_t len,
    const cadkit_options *options, char **out);
CADKIT_API cadkit_status cadkit_citygml_write_json(const char *json,
    const char *validation_options_json, char **out);

/* Releases a string returned by this library. NULL is a no-op. */
CADKIT_API void cadkit_string_free(char *s);

#ifdef __cplusplus
}
#endif

#endif /* CADKIT_H */
