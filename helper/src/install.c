/* Streaming a package to Sony's installer (KITE_HAVE_INSTALL on the PS5). */
#include "install.h"

#include <errno.h>
#include <stdio.h>
#include <string.h>

#ifdef KITE_HAVE_INSTALL
#include "sceAppInstUtil.h"
#endif

#define URL_MAX 2048
#define NAME_MAX 256

static void reason(char *msg, size_t cap, const char *text) {
    if (msg && cap) snprintf(msg, cap, "%s", text);
}

int kite_install_url(const char *url, size_t url_len, const char *name, size_t name_len,
                     char *msg, size_t msg_cap) {
    if (msg && msg_cap) msg[0] = '\0';
    if (!url || url_len == 0 || url_len >= URL_MAX) {
        reason(msg, msg_cap, "a package URL is needed");
        return -EINVAL;
    }
    char url_buf[URL_MAX];
    memcpy(url_buf, url, url_len);
    url_buf[url_len] = '\0';
    char name_buf[NAME_MAX];
    if (name_len >= sizeof name_buf) name_len = sizeof name_buf - 1;
    if (name && name_len) memcpy(name_buf, name, name_len);
    name_buf[name_len] = '\0';

#ifdef KITE_HAVE_INSTALL
    static int init_done = 0;
    static int init_rc = 0;
    if (!init_done) {
        init_rc = sceAppInstUtilInitialize();
        init_done = 1;
    }
    if (init_rc != 0) {
        snprintf(msg, msg_cap, "Sony's installer did not start (0x%08X)", (unsigned)init_rc);
        return init_rc;
    }
    MetaInfo meta;
    SceAppInstallPkgInfo pkg_info;
    PlayGoInfo playgo;
    memset(&meta, 0, sizeof meta);
    memset(&pkg_info, 0, sizeof pkg_info);
    memset(&playgo, 0, sizeof playgo);
    meta.uri = url_buf;
    meta.ex_uri = "";
    meta.playgo_scenario_id = "";
    meta.content_id = "";
    meta.content_name = name_buf;
    meta.icon_url = "";
    int rc = sceAppInstUtilInstallByPackage(&meta, &pkg_info, &playgo);
    if (rc != 0) snprintf(msg, msg_cap, "Sony's installer refused it (0x%08X)", (unsigned)rc);
    return rc;
#else
    (void)name_buf;
    reason(msg, msg_cap, "this helper was built without the installer");
    return -ENOSYS;
#endif
}
