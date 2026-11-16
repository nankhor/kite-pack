#ifndef KITE_INSTALL_H
#define KITE_INSTALL_H

#include <stddef.h>

/* Installs a package the console fetches from `url` through Sony's own installer
 * (sceAppInstUtilInstallByPackage). Sony's code comes back in the return value; a
 * negative result is the helper's own. `msg` gets a short reason for the screen. */
int kite_install_url(const char *url, size_t url_len, const char *name, size_t name_len,
                     char *msg, size_t msg_cap);

#endif /* KITE_INSTALL_H */
