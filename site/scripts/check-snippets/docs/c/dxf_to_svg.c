#include <stdio.h>
#include "cadkit.h"

int main(void) {
    cadkit_document *doc = NULL;
    if (cadkit_read_file("plan.dxf", NULL, &doc) != CADKIT_OK) {
        fprintf(stderr, "%s\n", cadkit_last_error_message());
        return 1;
    }
    cadkit_svg_options options;
    cadkit_svg_options_init(&options); /* always start from the defaults */
    options.model_index = 0;           /* model space; paper layouts follow */
    options.width_px = 1600;
    options.stroke_px = 1.0;
    options.background = NULL;         /* transparent; or a CSS colour such as "#ffffff" */
    options.text = 1;

    char *svg = NULL;
    int rc = 1;
    if (cadkit_document_to_svg(doc, &options, &svg) == CADKIT_OK) {
        FILE *out = fopen("plan.svg", "wb");
        if (out) {
            fputs(svg, out);
            fclose(out);
            rc = 0;
        }
        cadkit_string_free(svg);
    } else {
        fprintf(stderr, "%s\n", cadkit_last_error_message());
    }
    cadkit_document_free(doc);
    return rc;
}
