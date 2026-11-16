#include <stdio.h>
#include "cadkit.h"

int main(void) {
    cadkit_options options;
    cadkit_options_init(&options); /* defaults; zero selects the default of a field */
    options.max_input_bytes = 64ull << 20;
    options.max_decompressed_bytes = 256ull << 20;
    options.max_objects = 500000;
    options.max_depth = 32;
    options.max_string_bytes = 4u << 20;
    options.max_vertices = 1000000;

    cadkit_document *doc = NULL;
    cadkit_status st = cadkit_read_file("upload.dxf", &options, &doc);
    if (st == CADKIT_LIMIT_EXCEEDED) {
        fprintf(stderr, "rejected, over a limit: %s\n", cadkit_last_error_message());
        return 1;
    }
    if (st != CADKIT_OK) {
        fprintf(stderr, "rejected: %s: %s\n", cadkit_status_string(st), cadkit_last_error_message());
        return 1;
    }
    cadkit_info info;
    if (cadkit_document_info(doc, &info) == CADKIT_OK)
        printf("%llu entities, %llu warnings\n", (unsigned long long)info.entity_count,
               (unsigned long long)info.warning_count);
    cadkit_document_free(doc);
    return 0;
}
