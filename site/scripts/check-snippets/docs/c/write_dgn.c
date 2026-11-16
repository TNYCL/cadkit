#include <stdio.h>
#include <stdlib.h>
#include "cadkit.h"

/* Reads a whole file into memory; returns NULL on failure. */
static uint8_t *load(const char *path, size_t *len) {
    FILE *f = fopen(path, "rb");
    if (!f) return NULL;
    uint8_t *data = NULL;
    if (fseek(f, 0, SEEK_END) == 0) {
        long size = ftell(f);
        if (size > 0 && fseek(f, 0, SEEK_SET) == 0 && (data = malloc((size_t)size)) != NULL) {
            *len = fread(data, 1, (size_t)size, f);
        }
    }
    fclose(f);
    return data;
}

int main(void) {
    size_t template_len = 0;
    uint8_t *template_bytes = load("template.dgn", &template_len);
    if (!template_bytes) return 1;

    /* 1. A seed without the template's drawing content; the report counts what remains. */
    uint8_t *seed = NULL;
    size_t seed_len = 0;
    char *report = NULL;
    cadkit_status st = cadkit_dgn_strip_seed(template_bytes, template_len, "{\"model_name\":\"Model\"}",
                                             NULL, &seed, &seed_len, &report);
    free(template_bytes);
    if (st != CADKIT_OK) {
        fprintf(stderr, "%s\n", cadkit_last_error_message());
        return 1;
    }
    printf("%s\n", report);
    cadkit_string_free(report);

    /* 2. The C API has no seed preparation, so the document must already match the seed:
       one model, the seed's units and dimension, colours as seed palette indices, measured
       text. A DGN V8 drawing written for the same seed family does. */
    cadkit_document *doc = NULL;
    uint8_t *dgn = NULL;
    size_t dgn_len = 0;
    int rc = 1;
    if (cadkit_read_file("drawing.dgn", NULL, &doc) == CADKIT_OK &&
        cadkit_document_to_dgn(doc, seed, seed_len, NULL, &dgn, &dgn_len) == CADKIT_OK) {
        FILE *out = fopen("converted.dgn", "wb");
        if (out) {
            rc = fwrite(dgn, 1, dgn_len, out) == dgn_len ? 0 : 1;
            fclose(out);
        }
    } else {
        fprintf(stderr, "%s\n", cadkit_last_error_message());
    }
    cadkit_bytes_free(dgn); /* binary buffers: cadkit_bytes_free, never cadkit_string_free */
    cadkit_bytes_free(seed);
    cadkit_document_free(doc);
    return rc;
}
