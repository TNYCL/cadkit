#include <stdio.h>
#include "cadkit.h"

int main(void) {
    cadkit_document *doc = NULL;
    char *dxf = NULL;
    if (cadkit_read_file("plan.dxf", NULL, &doc) != CADKIT_OK) return 1; /* ASCII or binary */
    if (cadkit_document_to_dxf(doc, CADKIT_DXF_R2000, &dxf) == CADKIT_OK) {
        FILE *out = fopen("plan-r2000.dxf", "wb");
        if (out) {
            fputs(dxf, out);
            fclose(out);
        }
        cadkit_string_free(dxf);
    }
    cadkit_document_free(doc);
    return 0;
}
