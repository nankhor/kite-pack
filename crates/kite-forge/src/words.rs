//! Forge's failures in the words the screen shows: no "preflight failed:", sizes in GB.

const MIB: u64 = 1024 * 1024;

/// Decimal sizes with one decimal, as the screen counts them ("592.4 GB", "10.5 MB").
fn size(bytes: u64) -> String {
    match bytes {
        b if b >= 1_000_000_000 => format!("{:.1} GB", b as f64 / 1e9),
        b if b >= 1_000_000 => format!("{:.1} MB", b as f64 / 1e6),
        b if b >= 1_000 => format!("{} KB", (b as f64 / 1e3).round()),
        b => format!("{b} bytes"),
    }
}

/// The last folder of a path, whichever way its separators lean.
fn folder_name(dir: &str) -> &str {
    let dir = dir.trim_end_matches(['/', '\\']);
    dir.rsplit(['/', '\\']).next().filter(|n| !n.is_empty()).unwrap_or(dir)
}

/// Forge's "not enough space in <dir>: about <n> MiB needed, <m> MiB free", or None.
fn space(finding: &str) -> Option<String> {
    let rest = finding.strip_prefix("not enough space in ")?;
    let (dir, sizes) = rest.split_once(": about ")?;
    let (need, rest) = sizes.split_once(" MiB needed, ")?;
    let (free, _) = rest.split_once(" MiB free")?;
    let need: u64 = need.trim().parse().ok()?;
    let free: u64 = free.trim().parse().ok()?;
    Some(format!(
        "Not enough free space in {}: needs about {}, {} free",
        folder_name(dir),
        size(need * MIB),
        size(free * MIB)
    ))
}

/// One of Forge's findings as a sentence: its space finding reworded, any other with a capital.
fn finding(text: &str) -> String {
    let text = text.trim();
    space(text).unwrap_or_else(|| {
        let mut chars = text.chars();
        chars.next().map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
    })
}

/// Whether an error is the drive filling up: the OS's own code, as its message is in the
/// user's language (ENOSPC; ERROR_DISK_FULL and ERROR_HANDLE_DISK_FULL on Windows).
fn disk_full(why: &str) -> bool {
    let codes: &[&str] = if cfg!(windows) { &["(os error 112)", "(os error 39)"] } else { &["(os error 28)"] };
    codes.iter().any(|code| why.contains(code))
}

/// Why a pack failed, for the job's note. Forge's preflight lists what it found, one per line,
/// under "preflight failed:"; anything else is kept as it is.
pub(crate) fn plain(why: &str) -> String {
    if disk_full(why) {
        return "The drive filled up while packing: the game shrank less than expected".into();
    }
    match why.strip_prefix("preflight failed:") {
        Some(found) => {
            let found: Vec<String> = found.lines().map(str::trim).filter(|l| !l.is_empty()).map(finding).collect();
            if found.is_empty() { "Forge refused the pack".into() } else { found.join(" · ") }
        }
        None => why.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn says_how_much_space_in_gb() {
        let forge = "preflight failed:\n  not enough space in C:\\Users\\parwe\\Documents\\Games: about 564973 MiB needed, 307145 MiB free";
        assert_eq!(plain(forge), "Not enough free space in Games: needs about 592.4 GB, 322.1 GB free");
        let unix = "preflight failed:\n  not enough space in /Volumes/Fast/Packed/: about 1024 MiB needed, 10 MiB free";
        assert_eq!(plain(unix), "Not enough free space in Packed: needs about 1.1 GB, 10.5 MB free");
    }

    #[test]
    fn keeps_every_finding_on_one_line() {
        let forge = "preflight failed:\n  not enough space in /G: about 2048 MiB needed, 1024 MiB free\n  /G is FAT32, which cannot hold a file over 4 GiB (5000000000 bytes needed)";
        assert_eq!(
            plain(forge),
            "Not enough free space in G: needs about 2.1 GB, 1.1 GB free · /G is FAT32, which cannot hold a file over 4 GiB (5000000000 bytes needed)"
        );
        assert_eq!(plain("preflight failed:\n  the source has no eboot.bin"), "The source has no eboot.bin");
    }

    #[test]
    fn says_when_the_drive_filled_up() {
        let full = if cfg!(windows) {
            "writing ampr_zstd-071.pak: Espace insuffisant sur le disque. (os error 112)"
        } else {
            "writing ampr_zstd-071.pak: No space left on device (os error 28)"
        };
        assert_eq!(plain(full), "The drive filled up while packing: the game shrank less than expected");
    }

    #[test]
    fn leaves_other_failures_alone() {
        assert_eq!(plain("Couldn't create /x: denied"), "Couldn't create /x: denied");
        assert_eq!(plain("preflight failed:"), "Forge refused the pack");
    }
}
