#include <stdio.h>
#include "cadkit.h"

int main(void) {
    cadkit_document *doc = NULL;
    if (cadkit_read_file("plan.dgn", NULL, &doc) != CADKIT_OK) return 1; /* V7 or V8 */
    cadkit_info info;
    if (cadkit_document_info(doc, &info) == CADKIT_OK)
        printf("format %d, %llu levels\n", (int)info.format, (unsigned long long)info.layer_count);
    cadkit_document_free(doc);
    return 0;
}
