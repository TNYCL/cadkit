#include <stdio.h>
#include "cadkit.h"

int main(int argc, char **argv) {
    if (argc < 2) {
        fprintf(stderr, "usage: inspect_drawing <file>\n");
        return 2;
    }
    char *header = NULL; /* header only: no entity decoding */
    if (cadkit_inspect_file(argv[1], NULL, &header) == CADKIT_OK) {
        printf("%s\n", header);
        cadkit_string_free(header);
    }

    cadkit_document *doc = NULL;
    if (cadkit_read_file(argv[1], NULL, &doc) != CADKIT_OK) {
        fprintf(stderr, "%s\n", cadkit_last_error_message());
        return 1;
    }
    /* Counts, the entity histogram per kind and the warning count as JSON. Layer names and
       states are in the full document JSON (cadkit_document_to_json). */
    char *summary = NULL;
    if (cadkit_document_info_json(doc, &summary) == CADKIT_OK) {
        printf("%s\n", summary);
        cadkit_string_free(summary);
    }
    cadkit_document_free(doc);
    return 0;
}
