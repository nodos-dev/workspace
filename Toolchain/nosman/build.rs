extern crate vergen;
extern crate anyhow;

use anyhow::Result;
use std::fs;
use std::path::Path;
use vergen::{vergen, Config};

fn main() -> Result<()> {
    sync_vendored_cmake()?;

    // Generate the default 'cargo:' instruction output
    vergen(Config::default())
}

/// Mirrors the canonical workspace `../CMake` toolchain into the crate-local
/// `CMake/` directory so it ships inside the published crate (cargo only packages
/// files under the crate root). When the canonical source is absent — e.g. when
/// building the packaged tarball during `cargo publish`/`cargo install` — this is
/// a no-op and the already-vendored copy is used as-is.
fn sync_vendored_cmake() -> Result<()> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")?;
    let canonical = Path::new(&manifest_dir).join("..").join("CMake");
    let vendored = Path::new(&manifest_dir).join("data").join("cmake-toolchain");

    println!("cargo:rerun-if-changed={}", canonical.display());

    if !canonical.is_dir() {
        return Ok(());
    }

    sync_dir(&canonical, &vendored)?;
    Ok(())
}

/// Recursively syncs `src` into `dst`: copies files whose contents differ,
/// creates missing directories, and prunes entries in `dst` not present in `src`.
fn sync_dir(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;

    // Copy/update entries from src.
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            sync_dir(&from, &to)?;
        } else {
            let differs = match fs::read(&to) {
                Ok(existing) => existing != fs::read(&from)?,
                Err(_) => true,
            };
            if differs {
                fs::copy(&from, &to)?;
            }
        }
    }

    // Prune entries in dst that no longer exist in src.
    for entry in fs::read_dir(dst)? {
        let entry = entry?;
        let name = entry.file_name();
        if !src.join(&name).exists() {
            let path = entry.path();
            if path.is_dir() {
                fs::remove_dir_all(&path)?;
            } else {
                fs::remove_file(&path)?;
            }
        }
    }

    Ok(())
}
