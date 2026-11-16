#include <stdio.h>
#include "cadkit.h"

int main(void) {
    cadkit_document *doc = NULL;
    if (cadkit_read_file("cadkit-sample-plan.dxf", NULL, &doc) != CADKIT_OK) {
        fprintf(stderr, "%s\n", cadkit_last_error_message());
        return 1;
    }
    /* Layer names are in the document JSON (cadkit_document_to_json). */
    cadkit_info info;
    if (cadkit_document_info(doc, &info) == CADKIT_OK)
        printf("%llu layers\n", (unsigned long long)info.layer_count);

    cadkit_svg_options options;
    cadkit_svg_options_init(&options);
    options.width_px = 1200;
    char *svg = NULL;
    if (cadkit_document_to_svg(doc, &options, &svg) == CADKIT_OK) {
        FILE *out = fopen("plan.svg", "w");
        if (out) {
            fputs(svg, out);
            fclose(out);
        }
        cadkit_string_free(svg);
    }
    cadkit_document_free(doc);
    return 0;
}
