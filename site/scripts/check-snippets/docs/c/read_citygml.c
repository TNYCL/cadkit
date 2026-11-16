#include <stdio.h>
#include "cadkit.h"

int main(void) {
    cadkit_document *doc = NULL; /* neutral geometry view of the CityGML file */
    if (cadkit_read_file("building.gml", NULL, &doc) != CADKIT_OK) return 1;
    cadkit_info info;
    if (cadkit_document_info(doc, &info) == CADKIT_OK)
        printf("%llu entities\n", (unsigned long long)info.entity_count);
    cadkit_document_free(doc);
    return 0;
}
