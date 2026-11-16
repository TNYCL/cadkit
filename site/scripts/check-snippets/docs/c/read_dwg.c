#include <stdio.h>
#include "cadkit.h"

int main(void) {
    cadkit_document *doc = NULL;
    if (cadkit_read_file("plan.dwg", NULL, &doc) != CADKIT_OK) return 1;
    cadkit_info info;
    if (cadkit_document_info(doc, &info) == CADKIT_OK)
        printf("%llu layers\n", (unsigned long long)info.layer_count);
    cadkit_document_free(doc);
    return 0;
}
