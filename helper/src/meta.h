/* The Kite helper's read-only console metadata calls.
 *
 * Kite Pack needs to know what is installed on the PS5 to show a library, to skip a game that
 * is already there, and to tell which DLC the console already has. None of that needs Sony's
 * APIs: an installed title is a folder under /user/appmeta and /user/app, its name is a
 * param.json, its tile is an icon0.png, and its add-ons are folders under /user/addcont.
 * So these are plain filesystem calls with a read-only policy, not AppInstUtil calls.
 *
 * Only files under the read roots the helper was started with are reachable, and a path is
 * resolved with realpath() before the policy sees it, so a symlink cannot lead out of them.
 * Every call is bounded (entries, bytes, folders, wall time) and every walk skips symbolic
 * links, so a hostile or broken console tree cannot wedge a request.
 *
 * The bodies are little-endian binary (the same shape as AVA1's own codecs) rather than JSON:
 * the helper has no JSON parser, and a fixed layout is as easy to bound on both sides.
 * `kite-ps5/src/meta.rs` is the host half of this contract and documents it in Rust.
 *
 *   request  (all methods): u16 path_len | path bytes (no NUL) | u64 offset | u32 limit
 *
 *   fs.list    -> u32 count | count x { u16 name_len | name | u8 kind | u64 size | i64 mtime }
 *                 | u8 more | u64 next
 *   fs.stat    -> u8 kind | u64 size | i64 mtime | u32 mode
 *   fs.read    -> u32 len | u8 eof | bytes[len]
 *   fs.du      -> u64 bytes | u64 files | u64 dirs | u8 partial
 *   fs.freespace -> u64 total | u64 free | u64 usable | u64 reserve
 *
 * kind: 1 file, 2 directory, 3 symbolic link, 4 other.
 * Status: AVA1_STATUS_OK, AVA1_ERR_PATH (refused path), KITE_ERR_NOT_FOUND (no such path),
 * AVA1_ERR_IO (a real failure), AVA1_ERR_INTERNAL (the reply did not fit).
 */
#ifndef KITE_META_H
#define KITE_META_H

#include <stddef.h>
#include <stdint.h>

/* Application method numbers. AVA1 keeps 1-3 for the protocol, 16-23 for the data plane and
 * jobs; from 4 up a node serves its own (SPEC.md §7.3). These are ours and never move. */
#define KITE_METHOD_FS_LIST 24
#define KITE_METHOD_FS_STAT 25
#define KITE_METHOD_FS_READ 26
#define KITE_METHOD_FS_DU 27
#define KITE_METHOD_FS_FREESPACE 28

/* A path that is not there, told apart from a path that is there and cannot be read: the app
 * treats "/mnt/ext0/user/app does not exist" as "no games on that drive", not as an error. */
#define KITE_ERR_NOT_FOUND 40

/* The most this layer accepts or returns in one call. */
#define KITE_LIST_MAX 256u        /* entries per fs.list, like the data plane's pages */
#define KITE_READ_MAX (256u * 1024u)
#define KITE_DU_MAX_DIRS 200000u
#define KITE_DU_MAX_MS 10000u     /* a walk that runs this long says `partial` and stops */
#define KITE_NAME_MAX 255u

/* One directory entry, as fs.list reports it. */
typedef struct {
    char name[KITE_NAME_MAX + 1];
    uint8_t kind;
    uint64_t size;
    int64_t mtime;
} kite_entry_t;

/* What a walk summed. */
typedef struct {
    uint64_t bytes;  /* st_size of regular files, as drives count them */
    uint64_t files;
    uint64_t dirs;
    int partial;     /* a cap stopped it, so the numbers are a lower bound */
} kite_usage_t;

/* `root` is a test hook: it is prefixed to every request path, so a host test can stand a
 * folder up as a console root. "" on the PS5. `allowed` decides whether a resolved absolute
 * path may be read; the helper passes its read policy. Call before the server starts. */
void kite_meta_init(const char *root, int (*allowed)(const char *real));

/* Is `method` one of ours? */
int kite_meta_serves(uint16_t method);

/* Handles one of the methods above. Returns AVA1_STATUS_OK or an AVA1_ERR_*, or -1 when the
 * method is not ours (so the helper's own handler runs). */
int kite_meta_rpc(uint16_t method, const uint8_t *body, uint32_t len, uint8_t *out, size_t cap,
                  size_t *out_len);

#endif /* KITE_META_H */
