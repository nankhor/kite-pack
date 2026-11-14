/* Read-only console metadata: see meta.h for the methods, the wire layout and the policy. */
#include "meta.h"

#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/statvfs.h>
#include <time.h>
#include <unistd.h>

#include "ava1_data.h"
#include "ava1_gen.h"

#ifndef PATH_MAX
#define PATH_MAX 4096
#endif

/* The console's root, "" on the PS5; a folder in host tests. */
static char g_root[PATH_MAX];
static int (*g_allowed)(const char *real);

void kite_meta_init(const char *root, int (*allowed)(const char *real)) {
    g_root[0] = '\0';
    if (root && root[0]) {
        char real[PATH_MAX];
        /* A test root may not exist yet; keep it as given then. */
        snprintf(g_root, sizeof g_root, "%s", realpath(root, real) ? real : root);
    }
    g_allowed = allowed;
}

int kite_meta_serves(uint16_t method) {
    switch (method) {
    case KITE_METHOD_FS_LIST:
    case KITE_METHOD_FS_STAT:
    case KITE_METHOD_FS_READ:
    case KITE_METHOD_FS_DU:
    case KITE_METHOD_FS_FREESPACE:
        return 1;
    default:
        return 0;
    }
}

/* ---- little-endian, bounded both ways -------------------------------------------------------- */

struct rd {
    const uint8_t *p;
    size_t n, off;
    int bad;
};

static uint64_t rd_take(struct rd *r, size_t n) {
    if (r->bad || n > r->n - r->off) {
        r->bad = 1;
        return 0;
    }
    uint64_t v = 0;
    for (size_t i = 0; i < n; i++) v |= (uint64_t)r->p[r->off + i] << (8 * i);
    r->off += n;
    return v;
}

static uint16_t rd_u16(struct rd *r) { return (uint16_t)rd_take(r, 2); }
static uint32_t rd_u32(struct rd *r) { return (uint32_t)rd_take(r, 4); }
static uint64_t rd_u64(struct rd *r) { return rd_take(r, 8); }

struct wr {
    uint8_t *p;
    size_t cap, n;
    int full;
};

static void wr_take(struct wr *w, uint64_t v, size_t n) {
    if (w->full || n > w->cap - w->n) {
        w->full = 1;
        return;
    }
    for (size_t i = 0; i < n; i++) w->p[w->n + i] = (uint8_t)(v >> (8 * i));
    w->n += n;
}

static void wr_u8(struct wr *w, uint8_t v) { wr_take(w, v, 1); }
static void wr_u16(struct wr *w, uint16_t v) { wr_take(w, v, 2); }
static void wr_u32(struct wr *w, uint32_t v) { wr_take(w, v, 4); }
static void wr_u64(struct wr *w, uint64_t v) { wr_take(w, v, 8); }
static void wr_i64(struct wr *w, int64_t v) { wr_take(w, (uint64_t)v, 8); }

static void wr_bytes(struct wr *w, const void *p, size_t n) {
    if (w->full || n > w->cap - w->n) {
        w->full = 1;
        return;
    }
    memcpy(w->p + w->n, p, n);
    w->n += n;
}

/* ---- paths ------------------------------------------------------------------------------------ */

/* Free whose component is exactly "..", the way the data plane's path policy does. */
static int has_dotdot(const char *p) {
    const char *s = p;
    while (*s) {
        const char *e = strchr(s, '/');
        size_t n = e ? (size_t)(e - s) : strlen(s);
        if (n == 2 && s[0] == '.' && s[1] == '.') return 1;
        if (!e) break;
        s = e + 1;
    }
    return 0;
}

/* Turns a request path into the real absolute path to act on, or an error status.
 *
 * `out` is the path to open: the test root plus the console path, resolved. The policy then
 * sees the real path, so a symlink pointing outside the read roots is refused rather than
 * followed into a folder the helper was never given. */
static int resolve(const uint8_t *raw, size_t len, char *out, size_t cap) {
    char req[PATH_MAX], joined[PATH_MAX], real[PATH_MAX];
    if (len == 0 || len >= sizeof req || len >= cap) return AVA1_ERR_PATH;
    if (raw[0] != '/') return AVA1_ERR_PATH;
    if (memchr(raw, '\0', len) != NULL) return AVA1_ERR_PATH;
    memcpy(req, raw, len);
    req[len] = '\0';
    if (has_dotdot(req)) return AVA1_ERR_PATH;

    int n = snprintf(joined, sizeof joined, "%s%s", g_root, req);
    if (n < 0 || (size_t)n >= sizeof joined) return AVA1_ERR_PATH;
    if (!realpath(joined, real)) return errno == ENOENT || errno == ENOTDIR ? KITE_ERR_NOT_FOUND : AVA1_ERR_IO;
    if (g_allowed && !g_allowed(real)) return AVA1_ERR_PATH;
    snprintf(out, cap, "%s", real);
    return AVA1_STATUS_OK;
}

/* ---- requests --------------------------------------------------------------------------------- */

struct request {
    uint8_t path[PATH_MAX];
    size_t path_len;
    uint64_t offset;
    uint32_t limit;
};

/* The one request layout every method takes (meta.h). `limit` is the caller's; each method
 * clamps it, so an oversized value cannot be used to ask for more than the reply can hold. */
static int parse_request(const uint8_t *body, uint32_t len, struct request *r) {
    struct rd rd = { body, len, 0, 0 };
    size_t n = rd_u16(&rd);
    if (rd.bad || n == 0 || n >= sizeof r->path || n > rd.n - rd.off) return AVA1_ERR_PROTOCOL;
    memcpy(r->path, body + rd.off, n);
    rd.off += n;
    r->path_len = n;
    r->offset = rd_u64(&rd);
    r->limit = rd_u32(&rd);
    return rd.bad ? AVA1_ERR_PROTOCOL : AVA1_STATUS_OK;
}

/* ---- fs.list ---------------------------------------------------------------------------------- */

static int by_name(const void *a, const void *b) {
    const kite_entry_t *x = a, *y = b;
    return strcmp(x->name, y->name);
}

/* Every entry of a folder, sorted, capped at KITE_LIST_MAX * 256 so one absurd folder cannot
 * eat the helper's memory. The caller pages the sorted array, so a page is stable while a
 * folder is not changing under it. */
static int read_folder(const char *real, kite_entry_t **out, size_t *count, int *overflow) {
    DIR *d = opendir(real);
    if (!d) return errno == ENOENT ? KITE_ERR_NOT_FOUND : AVA1_ERR_IO;

    const size_t max = (size_t)KITE_LIST_MAX * 256u;
    kite_entry_t *v = malloc(sizeof *v * (max + 1));
    if (!v) {
        closedir(d);
        return AVA1_ERR_INTERNAL;
    }

    size_t n = 0;
    struct dirent *de;
    while ((de = readdir(d)) != NULL) {
        if (!strcmp(de->d_name, ".") || !strcmp(de->d_name, "..")) continue;
        if (strlen(de->d_name) > KITE_NAME_MAX) continue;
        if (n == max) {
            *overflow = 1;
            break;
        }
        struct stat st;
        if (fstatat(dirfd(d), de->d_name, &st, AT_SYMLINK_NOFOLLOW) != 0) continue;
        kite_entry_t *e = &v[n];
        snprintf(e->name, sizeof e->name, "%s", de->d_name);
        e->kind = S_ISREG(st.st_mode) ? 1 : S_ISDIR(st.st_mode) ? 2 : S_ISLNK(st.st_mode) ? 3 : 4;
        e->size = e->kind == 1 ? (uint64_t)st.st_size : 0;
        e->mtime = (int64_t)st.st_mtime;
        n++;
    }
    closedir(d);

    qsort(v, n, sizeof *v, by_name);
    *out = v;
    *count = n;
    return AVA1_STATUS_OK;
}

static int fs_list(const struct request *rq, uint8_t *out, size_t cap, size_t *out_len) {
    char real[PATH_MAX];
    int rc = resolve(rq->path, rq->path_len, real, sizeof real);
    if (rc != AVA1_STATUS_OK) return rc;

    kite_entry_t *v = NULL;
    size_t n = 0;
    int overflow = 0;
    rc = read_folder(real, &v, &n, &overflow);
    if (rc != AVA1_STATUS_OK) return rc;

    uint64_t from = rq->offset;
    size_t limit = rq->limit ? rq->limit : KITE_LIST_MAX;
    if (limit > KITE_LIST_MAX) limit = KITE_LIST_MAX;
    if (from > n) from = n;

    struct wr w = { out, cap, 0, 0 };
    size_t emitted = 0;
    for (size_t i = (size_t)from; i < n && emitted < limit; i++) {
        size_t len = strlen(v[i].name);
        wr_u16(&w, (uint16_t)len);
        wr_bytes(&w, v[i].name, len);
        wr_u8(&w, v[i].kind);
        wr_u64(&w, v[i].size);
        wr_i64(&w, v[i].mtime);
        if (w.full) break;
        emitted++;
    }
    int more = (size_t)from + emitted < n || overflow;
    wr_u8(&w, (uint8_t)(more ? 1 : 0));
    wr_u64(&w, from + emitted);
    free(v);

    if (w.full) return AVA1_ERR_INTERNAL;
    /* The count is a prefix, not a suffix: assemble it by shifting the entries up two bytes. */
    if (w.n + 4 > cap) return AVA1_ERR_INTERNAL;
    memmove(out + 4, out, w.n);
    out[0] = (uint8_t)(emitted & 0xff);
    out[1] = (uint8_t)((emitted >> 8) & 0xff);
    out[2] = (uint8_t)((emitted >> 16) & 0xff);
    out[3] = (uint8_t)((emitted >> 24) & 0xff);
    *out_len = w.n + 4;
    return AVA1_STATUS_OK;
}

/* ---- fs.stat ---------------------------------------------------------------------------------- */

static int fs_stat(const struct request *rq, uint8_t *out, size_t cap, size_t *out_len) {
    char real[PATH_MAX];
    int rc = resolve(rq->path, rq->path_len, real, sizeof real);
    if (rc != AVA1_STATUS_OK) return rc;

    struct stat st;
    if (stat(real, &st) != 0) return errno == ENOENT ? KITE_ERR_NOT_FOUND : AVA1_ERR_IO;

    struct wr w = { out, cap, 0, 0 };
    wr_u8(&w, S_ISREG(st.st_mode) ? 1 : S_ISDIR(st.st_mode) ? 2 : S_ISLNK(st.st_mode) ? 3 : 4);
    wr_u64(&w, S_ISREG(st.st_mode) ? (uint64_t)st.st_size : 0);
    wr_i64(&w, (int64_t)st.st_mtime);
    wr_u32(&w, (uint32_t)st.st_mode);
    if (w.full) return AVA1_ERR_INTERNAL;
    *out_len = w.n;
    return AVA1_STATUS_OK;
}

/* ---- fs.read ---------------------------------------------------------------------------------- */

static int fs_read(const struct request *rq, uint8_t *out, size_t cap, size_t *out_len) {
    char real[PATH_MAX];
    int rc = resolve(rq->path, rq->path_len, real, sizeof real);
    if (rc != AVA1_STATUS_OK) return rc;

    /* O_NONBLOCK so a FIFO named icon0.png cannot park the helper's worker. */
    int fd = open(real, O_RDONLY | O_NONBLOCK | O_CLOEXEC);
    if (fd < 0) return errno == ENOENT ? KITE_ERR_NOT_FOUND : AVA1_ERR_IO;

    struct stat st;
    if (fstat(fd, &st) != 0 || !S_ISREG(st.st_mode)) {
        close(fd);
        return AVA1_ERR_PATH;
    }

    uint64_t size = (uint64_t)st.st_size;
    uint64_t from = rq->offset;
    size_t want = rq->limit ? rq->limit : KITE_READ_MAX;
    if (want > KITE_READ_MAX) want = KITE_READ_MAX;
    if (cap < 5) {
        close(fd);
        return AVA1_ERR_INTERNAL;
    }
    if (want > cap - 5) want = cap - 5;
    if (from >= size) want = 0;
    else if (want > size - from) want = (size_t)(size - from);

    /* The reply is u32 len | u8 eof | bytes, so the bytes land at out + 5 and the two fields
     * are written in front of them once the count is known. */
    size_t got = 0;
    while (got < want) {
        ssize_t n = pread(fd, out + 5 + got, want - got, (off_t)(from + got));
        if (n < 0) {
            if (errno == EINTR) continue;
            close(fd);
            return AVA1_ERR_IO;
        }
        if (n == 0) break;
        got += (size_t)n;
    }
    close(fd);

    out[0] = (uint8_t)(got & 0xff);
    out[1] = (uint8_t)((got >> 8) & 0xff);
    out[2] = (uint8_t)((got >> 16) & 0xff);
    out[3] = (uint8_t)((got >> 24) & 0xff);
    out[4] = (uint8_t)(from + got >= size ? 1 : 0);
    *out_len = got + 5;
    return AVA1_STATUS_OK;
}

/* ---- fs.du ------------------------------------------------------------------------------------ */

struct walk {
    kite_usage_t sum;
    dev_t dev;
    uint64_t dirs_left;
    struct timespec deadline;
    int started;
};

static int past_deadline(struct walk *w) {
    if (!w->started) return 0;
    struct timespec now;
    clock_gettime(CLOCK_MONOTONIC, &now);
    return now.tv_sec > w->deadline.tv_sec ||
           (now.tv_sec == w->deadline.tv_sec && now.tv_nsec >= w->deadline.tv_nsec);
}

/* Sums a tree the way a drive does: st_size for regular files, nothing for links, and only
 * entries on the same device as the root (a mounted sub-volume is another drive's space, and
 * counting it would show a game as twice its size). Bounded by folders and wall time; a walk
 * that stops early is `partial`, never silently complete. */
static void walk_tree(struct walk *w, const char *real, int depth) {
    if (w->sum.partial) return;
    DIR *d = opendir(real);
    if (!d) {
        w->sum.partial = 1;
        return;
    }
    struct dirent *de;
    while ((de = readdir(d)) != NULL) {
        if (!strcmp(de->d_name, ".") || !strcmp(de->d_name, "..")) continue;
        if (past_deadline(w) || w->dirs_left == 0) {
            w->sum.partial = 1;
            break;
        }
        struct stat st;
        if (fstatat(dirfd(d), de->d_name, &st, AT_SYMLINK_NOFOLLOW) != 0) continue;
        if (st.st_dev != w->dev) continue;
        if (S_ISREG(st.st_mode)) {
            w->sum.bytes += (uint64_t)st.st_size;
            w->sum.files++;
        } else if (S_ISDIR(st.st_mode)) {
            if (depth >= 64) {
                w->sum.partial = 1;
                continue;
            }
            w->sum.dirs++;
            w->dirs_left--;
            char child[PATH_MAX];
            if (snprintf(child, sizeof child, "%s/%s", real, de->d_name) < (int)sizeof child) {
                walk_tree(w, child, depth + 1);
            }
        }
    }
    closedir(d);
}

static int fs_du(const struct request *rq, uint8_t *out, size_t cap, size_t *out_len) {
    char real[PATH_MAX];
    int rc = resolve(rq->path, rq->path_len, real, sizeof real);
    if (rc != AVA1_STATUS_OK) return rc;

    struct stat st;
    if (stat(real, &st) != 0) return errno == ENOENT ? KITE_ERR_NOT_FOUND : AVA1_ERR_IO;

    struct walk w;
    memset(&w, 0, sizeof w);
    w.dev = st.st_dev;
    w.dirs_left = KITE_DU_MAX_DIRS;
    clock_gettime(CLOCK_MONOTONIC, &w.deadline);
    w.deadline.tv_sec += KITE_DU_MAX_MS / 1000;
    w.deadline.tv_nsec += (long)(KITE_DU_MAX_MS % 1000) * 1000000L;
    if (w.deadline.tv_nsec >= 1000000000L) {
        w.deadline.tv_sec++;
        w.deadline.tv_nsec -= 1000000000L;
    }
    w.started = 1;

    if (S_ISREG(st.st_mode)) {
        w.sum.bytes = (uint64_t)st.st_size;
        w.sum.files = 1;
    } else if (S_ISDIR(st.st_mode)) {
        walk_tree(&w, real, 0);
    } else {
        return AVA1_ERR_PATH;
    }

    struct wr wr = { out, cap, 0, 0 };
    wr_u64(&wr, w.sum.bytes);
    wr_u64(&wr, w.sum.files);
    wr_u64(&wr, w.sum.dirs);
    wr_u8(&wr, (uint8_t)(w.sum.partial ? 1 : 0));
    if (wr.full) return AVA1_ERR_INTERNAL;
    *out_len = wr.n;
    return AVA1_STATUS_OK;
}

/* ---- fs.freespace ----------------------------------------------------------------------------- */

/* The reserve Sony keeps back: 1/64 of the volume, at most 1 GiB, so "free" is not the number
 * a game can actually use. An image mount has no reserve of its own; this only ever reports
 * what statvfs says, minus that. */
static int fs_freespace(const struct request *rq, uint8_t *out, size_t cap, size_t *out_len) {
    char real[PATH_MAX];
    int rc = resolve(rq->path, rq->path_len, real, sizeof real);
    if (rc != AVA1_STATUS_OK) return rc;

    struct statvfs fs;
    if (statvfs(real, &fs) != 0) return AVA1_ERR_IO;

    uint64_t total = (uint64_t)fs.f_blocks * fs.f_frsize;
    uint64_t free_bytes = (uint64_t)fs.f_bavail * fs.f_frsize;
    uint64_t reserve = total / 64;
    if (reserve > 1024ull * 1024 * 1024) reserve = 1024ull * 1024 * 1024;
    uint64_t usable = free_bytes > reserve ? free_bytes - reserve : 0;

    struct wr w = { out, cap, 0, 0 };
    wr_u64(&w, total);
    wr_u64(&w, free_bytes);
    wr_u64(&w, usable);
    wr_u64(&w, reserve);
    if (w.full) return AVA1_ERR_INTERNAL;
    *out_len = w.n;
    return AVA1_STATUS_OK;
}

/* ---- dispatch --------------------------------------------------------------------------------- */

int kite_meta_rpc(uint16_t method, const uint8_t *body, uint32_t len, uint8_t *out, size_t cap,
                  size_t *out_len) {
    if (!kite_meta_serves(method)) return -1;

    struct request rq;
    memset(&rq, 0, sizeof rq);
    int rc = parse_request(body, len, &rq);
    if (rc != AVA1_STATUS_OK) return rc;

    switch (method) {
    case KITE_METHOD_FS_LIST:
        return fs_list(&rq, out, cap, out_len);
    case KITE_METHOD_FS_STAT:
        return fs_stat(&rq, out, cap, out_len);
    case KITE_METHOD_FS_READ:
        return fs_read(&rq, out, cap, out_len);
    case KITE_METHOD_FS_DU:
        return fs_du(&rq, out, cap, out_len);
    case KITE_METHOD_FS_FREESPACE:
        return fs_freespace(&rq, out, cap, out_len);
    default:
        return AVA1_ERR_UNKNOWN_METHOD;
    }
}
