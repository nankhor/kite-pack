//! Embeds the Kite helper (kite-helper.elf) when KITE_HELPER_ELF names it; CI builds it with the
//! PS5 payload SDK. Without it the app still builds, and Start the Kite helper says so.
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=KITE_HELPER_ELF");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("kite-helper.elf");
    match std::env::var_os("KITE_HELPER_ELF").map(PathBuf::from) {
        Some(elf) => {
            println!("cargo:rerun-if-changed={}", elf.display());
            std::fs::copy(&elf, &out).unwrap_or_else(|e| panic!("KITE_HELPER_ELF {}: {e}", elf.display()));
        }
        None => std::fs::write(&out, b"").unwrap(),
    }
    tauri_build::build()
}
