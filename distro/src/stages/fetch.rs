use crate::config::Config;
use anyhow::{Context, Result};
use builder_core::stages::{already_built, run};
use std::process::Command;

/// Downloads and extracts the kernel source (reuses the same
/// `builder-core` helpers `distroless` uses, since the tarball-fetch shape
/// is identical). Userland sources (util-linux, shadow-utils, ...) land
/// here too once Phase 1 is implemented — see the distro roadmap.
pub fn fetch(cfg: &Config, force: bool) -> Result<()> {
    std::fs::create_dir_all(cfg.sources_dir()).context("creating sources dir")?;

    let extracted_dir = cfg.kernel_build_dir();
    if already_built(&extracted_dir, force) {
        println!("skip fetch: {} already extracted", extracted_dir.display());
        return Ok(());
    }

    let archive_name = format!("linux-{}.tar.xz", cfg.kernel.version);
    let archive_path = cfg.sources_dir().join(&archive_name);
    if !archive_path.exists() {
        println!("downloading {}", cfg.kernel.url);
        run(Command::new("wget").args(["-O", archive_path.to_str().unwrap(), &cfg.kernel.url]))?;
    }

    let parent = extracted_dir.parent().context("extracted dir has no parent")?;
    std::fs::create_dir_all(parent)?;

    println!("extracting {archive_name} into {}", parent.display());
    run(Command::new("tar").args([
        "-xf",
        archive_path.to_str().unwrap(),
        "-C",
        parent.to_str().unwrap(),
    ]))?;

    Ok(())
}
