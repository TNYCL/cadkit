/* Prints a summary of a DWG/DGN/DXF drawing. Usage: info <file> [out.svg] */
#include <stdio.h>
#include <stdlib.h>

#include "cadkit.h"

int main(int argc, char **argv) {
    if (argc < 2) {
        fprintf(stderr, "usage: %s <file> [out.svg]\n", argv[0]);
        return 2;
    }
    printf("cadkit %s\n", cadkit_version());

    cadkit_document *doc = NULL;
    cadkit_status st = cadkit_read_file(argv[1], NULL, &doc);
    if (st != CADKIT_OK) {
        fprintf(stderr, "read failed: %s (%s)\n", cadkit_status_string(st), cadkit_last_error_message());
        return 1;
    }

    cadkit_info info;
    if (cadkit_document_info(doc, &info) == CADKIT_OK) {
        printf("format=%d models=%llu layers=%llu blocks=%llu entities=%llu warnings=%llu\n",
               (int)info.format, (unsigned long long)info.model_count,
               (unsigned long long)info.layer_count, (unsigned long long)info.block_count,
               (unsigned long long)info.entity_count, (unsigned long long)info.warning_count);
    }

    char *json = NULL;
    if (cadkit_document_info_json(doc, &json) == CADKIT_OK) {
        printf("%s\n", json);
        cadkit_string_free(json);
    }

    if (argc > 2) {
        char *svg = NULL;
        st = cadkit_document_to_svg(doc, NULL, &svg);
        if (st == CADKIT_OK) {
            FILE *f = fopen(argv[2], "wb");
            if (f) {
                fputs(svg, f);
                fclose(f);
                printf("wrote %s\n", argv[2]);
            }
            cadkit_string_free(svg);
        } else {
            fprintf(stderr, "svg failed: %s\n", cadkit_last_error_message());
        }
    }

    cadkit_document_free(doc);
    return 0;
}
