/* The Kite helper: an AVA1 node on the PS5 that Kite Pack sends packed games to.
 *
 *   kite-helper [--state DIR] [--allow DIR]... [--port N] [--trust CIDR]...
 *
 * On the PS5 it runs with no arguments: state in /data/kite, clients may write under /data and
 * /mnt, port 9130, and computers on the home network (private addresses) are trusted with no
 * code. It answers Kite Pack's discovery broadcast on the same port number (UDP).
 *
 * Built on AVA1's example node (third_party/ava1, examples/c/node.c upstream). */
#include <arpa/inet.h>
#include <errno.h>
#include <limits.h>
#include <netinet/in.h>
#include <pthread.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <unistd.h>

#include "ava1_data.h"
#include "ava1_events.h"
#include "ava1_gen.h"
#include "ava1_server.h"
#include "ava1_store.h"
#include "ava1_wire.h"
#include "notify.h"

#ifndef KITE_VERSION
#define KITE_VERSION "0.0.0"
#endif
#ifndef KITE_PLATFORM
#define KITE_PLATFORM "host"
#endif

#define DEFAULT_PORT 9130
#define MAX_ALLOW 8
#define MAX_TRUST 8
#define DISCOVER "KITE-DISCOVER 1"

struct cidr {
    uint32_t net;  /* host order */
    uint32_t mask; /* host order */
};

static char g_allow[MAX_ALLOW][PATH_MAX];
static int g_nallow;
static struct cidr g_trust[MAX_TRUST];
static int g_ntrust;
static volatile sig_atomic_t g_stop;

/* ---- what clients may touch ------------------------------------------------------------------ */

static int under(const char *abs, const char *root) {
    size_t n = strlen(root);
    if (n > 0 && root[n - 1] == '/') n--;
    return strncmp(abs, root, n) == 0 && (abs[n] == '\0' || abs[n] == '/');
}

static int allowed(const char *abs) {
    for (int i = 0; i < g_nallow; i++)
        if (under(abs, g_allow[i])) return 1;
    return 0;
}

static int may_write(const char *abs) { return allowed(abs); }
static int may_read(const char *abs, int unsafe_read) { (void)unsafe_read; return allowed(abs); }

static int same_device(const char *a, const char *b) {
    struct stat sa, sb;
    if (stat(a, &sa) != 0 || stat(b, &sb) != 0) return -1;
    return sa.st_dev == sb.st_dev;
}

/* ---- who may connect: the home network, with no code ----------------------------------------- */

static int parse_cidr(const char *s, struct cidr *out) {
    char buf[64];
    const char *slash = strchr(s, '/');
    int bits = 32;
    struct in_addr a;
    size_t n = slash ? (size_t)(slash - s) : strlen(s);
    if (n == 0 || n >= sizeof buf) return -1;
    memcpy(buf, s, n);
    buf[n] = '\0';
    if (inet_pton(AF_INET, buf, &a) != 1) return -1;
    if (slash) {
        bits = atoi(slash + 1);
        if (bits < 0 || bits > 32) return -1;
    }
    out->mask = bits == 0 ? 0 : 0xFFFFFFFFu << (32 - bits);
    out->net = ntohl(a.s_addr) & out->mask;
    return 0;
}

static void trust(const char *s) {
    if (g_ntrust < MAX_TRUST && parse_cidr(s, &g_trust[g_ntrust]) == 0) g_ntrust++;
    else fprintf(stderr, "kite helper: ignoring --trust %s\n", s);
}

/* A computer seen for the first time: trusted at once from a home-network address, refused
 * otherwise. `ip` is in network byte order, as AVA1 passes it. */
static int admit_new(const uint8_t key[32], const char *peer_name, uint32_t ip) {
    uint32_t h = ntohl(ip);
    (void)key;
    for (int i = 0; i < g_ntrust; i++) {
        if ((h & g_trust[i].mask) == g_trust[i].net) {
            printf("trusted %s\n", peer_name);
            fflush(stdout);
            return 1;
        }
    }
    return -1;
}

/* ---- node.info ------------------------------------------------------------------------------- */

static int rpc(uint16_t method, const uint8_t *body, uint32_t len, uint8_t *out, size_t cap, size_t *out_len) {
    ava1_node_info_t ni;
    ava1_w_t w;
    int rc = ava1_data_rpc(method, body, len, out, cap, out_len);
    if (rc != -1) return rc;
    if (method != AVA1_METHOD_NODE_INFO) return AVA1_ERR_UNKNOWN_METHOD;
    memset(&ni, 0, sizeof ni);
    ni.version = (const uint8_t *)KITE_VERSION;
    ni.version_len = (uint32_t)strlen(KITE_VERSION);
    ni.platform = (const uint8_t *)KITE_PLATFORM;
    ni.platform_len = (uint32_t)strlen(KITE_PLATFORM);
    ni.name = (const uint8_t *)"Kite helper";
    ni.name_len = 11;
    ava1_w_init(&w, out, cap);
    if (ava1_node_info_encode(&ni, &w) != 0) return AVA1_ERR_INTERNAL;
    *out_len = w.len;
    return AVA1_STATUS_OK;
}

static void on_log(const char *msg) { fprintf(stderr, "%s\n", msg); }

/* ---- discovery: answer Kite Pack's broadcast ------------------------------------------------- */

/* The discovery socket, bound before the helper says it is listening, so an answer is ready the
 * moment anyone can find the helper. -1 if UDP is unavailable. */
static int discovery_socket(uint16_t port) {
    struct sockaddr_in a;
    int one = 1;
    int fd = socket(AF_INET, SOCK_DGRAM, 0);
    if (fd < 0) return -1;
    (void)setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &one, sizeof one);
    memset(&a, 0, sizeof a);
    a.sin_family = AF_INET;
    a.sin_addr.s_addr = htonl(INADDR_ANY);
    a.sin_port = htons(port);
    if (bind(fd, (struct sockaddr *)&a, sizeof a) != 0) {
        fprintf(stderr, "kite helper: discovery off (udp %u: %s)\n", (unsigned)port, strerror(errno));
        close(fd);
        return -1;
    }
    return fd;
}

struct discovery_arg {
    int fd;
    uint16_t port;
};

static void *discovery(void *p) {
    struct discovery_arg arg = *(struct discovery_arg *)p;
    struct sockaddr_in from;
    char buf[64], reply[96];
    int n_reply = snprintf(reply, sizeof reply, "KITE-HELPER 1 %u %s", (unsigned)arg.port, KITE_VERSION);
    free(p);
    while (!g_stop) {
        socklen_t fl = sizeof from;
        ssize_t n = recvfrom(arg.fd, buf, sizeof buf - 1, 0, (struct sockaddr *)&from, &fl);
        if (n <= 0) continue;
        buf[n] = '\0';
        if (strncmp(buf, DISCOVER, strlen(DISCOVER)) == 0)
            (void)sendto(arg.fd, reply, (size_t)n_reply, 0, (struct sockaddr *)&from, fl);
    }
    close(arg.fd);
    return NULL;
}

/* ---- start ----------------------------------------------------------------------------------- */

static void stop(int sig) { (void)sig; g_stop = 1; }

static void add_allow(const char *dir) {
    char real[PATH_MAX];
    mkdir(dir, 0755);
    if (g_nallow < MAX_ALLOW) {
        /* Clients send absolute paths as the console sees them; compare against the real one. */
        snprintf(g_allow[g_nallow++], PATH_MAX, "%s", realpath(dir, real) ? real : dir);
    }
}

int main(int argc, char **argv) {
    ava1_server_cfg_t cfg;
    ava1_data_cfg_t dc;
    char path[PATH_MAX + 32];
    const char *state = "/data/kite";
    unsigned port = DEFAULT_PORT;
    pthread_t disc;
    int rc;

    for (int i = 1; i < argc; i++) {
        if (!strcmp(argv[i], "--state") && i + 1 < argc) state = argv[++i];
        else if (!strcmp(argv[i], "--allow") && i + 1 < argc) add_allow(argv[++i]);
        else if (!strcmp(argv[i], "--port") && i + 1 < argc) port = (unsigned)atoi(argv[++i]);
        else if (!strcmp(argv[i], "--trust") && i + 1 < argc) trust(argv[++i]);
        else {
            fprintf(stderr, "usage: %s [--state DIR] [--allow DIR]... [--port N] [--trust CIDR]...\n", argv[0]);
            return 2;
        }
    }
    if (g_nallow == 0) {
        add_allow("/data");
        add_allow("/mnt");
    }
    if (g_ntrust == 0) {
        trust("10.0.0.0/8");
        trust("172.16.0.0/12");
        trust("192.168.0.0/16");
        trust("169.254.0.0/16");
        trust("127.0.0.0/8");
    }
    signal(SIGPIPE, SIG_IGN);
    mkdir(state, 0700);

    memset(&cfg, 0, sizeof cfg);
    cfg.port = (uint16_t)port;
    snprintf(path, sizeof path, "%s/identity", state);
    if (ava1_identity_load_or_create(path, &cfg.identity) != 0) {
        fprintf(stderr, "kite helper: cannot read or create %s\n", path);
        kite_notify("Kite helper couldn't start: its folder isn't writable.");
        return 1;
    }
    snprintf(cfg.name, sizeof cfg.name, "Kite helper");
    snprintf(cfg.peers_path, sizeof cfg.peers_path, "%s/peers", state);
    cfg.ping_every_ms = 2000;
    cfg.dead_after_ms = 12000;
    cfg.handshake_ms = 10000;
    cfg.admit_new = admit_new;
    cfg.log = on_log;
    cfg.rpc = rpc;

    memset(&dc, 0, sizeof dc);
    snprintf(dc.jobs_dir, sizeof dc.jobs_dir, "%s/jobs", state);
    mkdir(dc.jobs_dir, 0700);
    dc.may_write = may_write;
    dc.may_read = may_read;
    dc.same_device = same_device;
    snprintf(path, sizeof path, "%s/events.log", state);
    ava1_events_set_path(path);
    if ((rc = ava1_data_start(&dc)) != 0) {
        fprintf(stderr, "kite helper: data layer: %s\n", strerror(-rc));
        return 1;
    }
    cfg.data = ava1_data_hooks();
    cfg.caps = AVA1_CAP_DATA_PLANE;

    if ((rc = ava1_server_start(&cfg)) != 0) {
        fprintf(stderr, "kite helper: port %u: %s\n", port, strerror(-rc));
        kite_notify(rc == -EADDRINUSE ? "Kite helper is already running." : "Kite helper couldn't start.");
        ava1_data_stop();
        return 1;
    }
    port = ava1_server_port();
    {
        struct discovery_arg *arg = malloc(sizeof *arg);
        if (arg && (arg->fd = discovery_socket((uint16_t)port)) >= 0) {
            arg->port = (uint16_t)port;
            if (pthread_create(&disc, NULL, discovery, arg) == 0) pthread_detach(disc);
            else close(arg->fd), free(arg);
        } else {
            free(arg);
        }
    }
    printf("kite helper listening on %u\n", port);
    fflush(stdout);
    kite_notify("Kite helper is running.");

    signal(SIGINT, stop);
    signal(SIGTERM, stop);
    while (!g_stop) sleep(1);
    ava1_server_stop();
    ava1_data_stop();
    return 0;
}
