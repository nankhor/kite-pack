/* PS5 notifications. sceKernelSendNotificationRequest is looked up when first needed: a firmware
 * whose libkernel lacks it would otherwise stop the payload before main (the same approach as
 * ps5upload). Without it, nothing is shown. */
#include <dlfcn.h>
#include <stdio.h>
#include <string.h>

#include "notify.h"

typedef struct {
    char reserved[45];
    char message[3075];
} kite_notify_req_t;

typedef int (*send_notification_fn)(int, kite_notify_req_t *, size_t, int);

void kite_notify(const char *message) {
    static send_notification_fn send;
    static int looked;
    kite_notify_req_t req;
    if (!looked) {
        send = (send_notification_fn)dlsym(RTLD_DEFAULT, "sceKernelSendNotificationRequest");
        looked = 1;
    }
    if (!send) return;
    memset(&req, 0, sizeof req);
    snprintf(req.message, sizeof req.message, "%s", message);
    (void)send(0, &req, sizeof req, 0);
}
