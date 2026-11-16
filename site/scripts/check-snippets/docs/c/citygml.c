#include <stdio.h>
#include <string.h>
#include "cadkit.h"

/* The CRS your coordinates are in; cadkit never guesses or transforms one. */
#define CRS "urn:ogc:def:crs:EPSG::32635"

int main(void) {
    cadkit_document *doc = NULL;
    char *xml = NULL, *native = NULL, *preflight = NULL;
    int rc = 1;
    if (cadkit_read_file("surfaces.dxf", NULL, &doc) != CADKIT_OK ||
        cadkit_document_to_citygml(doc, CRS, 1, &xml) != CADKIT_OK) {
        fprintf(stderr, "%s\n", cadkit_last_error_message());
        goto done;
    }
    FILE *out = fopen("model.gml", "wb");
    if (out) {
        fputs(xml, out);
        fclose(out);
    }
    /* Native CityGML as editable JSON, then the local structural TKGM preflight. */
    if (cadkit_citygml_read_json((const uint8_t *)xml, strlen(xml), NULL, &native) == CADKIT_OK &&
        cadkit_citygml_preflight_tkgm(native, "city-model-tender", &preflight) == CADKIT_OK) {
        printf("%s\n", preflight); /* "acceptance_verified": false, issues, unchecked rules */
        rc = 0;
    } else {
        fprintf(stderr, "%s\n", cadkit_last_error_message());
    }
done:
    cadkit_string_free(preflight);
    cadkit_string_free(native);
    cadkit_string_free(xml);
    cadkit_document_free(doc);
    return rc;
}
