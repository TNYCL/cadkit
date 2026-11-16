#include <stdio.h>
#include "cadkit.h"

int main(void) {
    cadkit_document *doc = NULL;
    cadkit_status status = cadkit_read_file("plan.dxf", NULL, &doc);
    if (status != CADKIT_OK) {
        fprintf(stderr, "%s: %s\n", cadkit_status_string(status), cadkit_last_error_message());
        return 1;
    }
    cadkit_info info;
    if (cadkit_document_info(doc, &info) == CADKIT_OK)
        printf("%llu models, %llu layers, %llu entities\n", (unsigned long long)info.model_count,
               (unsigned long long)info.layer_count, (unsigned long long)info.entity_count);

    cadkit_svg_options options;
    cadkit_svg_options_init(&options);
    options.width_px = 1200;
    char *svg = NULL;
    if (cadkit_document_to_svg(doc, &options, &svg) == CADKIT_OK) {
        FILE *out = fopen("plan.svg", "wb");
        if (out) {
            fputs(svg, out);
            fclose(out);
        }
        cadkit_string_free(svg);
    }
    cadkit_document_free(doc);
    return 0;
}
