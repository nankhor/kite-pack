#include <stdio.h>

#include "notify.h"

void kite_notify(const char *message) {
    printf("notify: %s\n", message);
    fflush(stdout);
}
