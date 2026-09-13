use super::{already_built, run};
use crate::config::Config;
use anyhow::{Context, Result};
use std::process::Command;

pub fn fetch(cfg: &Config, force: bool, clean: bool) -> Result<()> {
    if clean {
        clean_sources(cfg)?;
    }

    std::fs::create_dir_all(cfg.sources_dir()).context("creating sources dir")?;

    fetch_tarball(
        cfg,
        &cfg.kernel.url,
        &format!("linux-{}.tar.xz", cfg.kernel.version),
        &cfg.kernel_build_dir(),
        force,
    )?;

    fetch_tarball(
        cfg,
        &cfg.busybox.url,
        &format!("busybox-{}.tar.bz2", cfg.busybox.version),
        &cfg.busybox_build_dir(),
        force,
    )?;

    fetch_uutils(cfg, force)?;

    Ok(())
}

/// Remove downloaded archives and extracted source trees so the next fetch
/// starts from scratch.
fn clean_sources(cfg: &Config) -> Result<()> {
    for dir in [
        cfg.sources_dir(),
        cfg.kernel_build_dir(),
        cfg.busybox_build_dir(),
        cfg.uutils_build_dir(),
    ] {
        if dir.exists() {
            println!("removing {}", dir.display());
            std::fs::remove_dir_all(&dir)
                .with_context(|| format!("removing {}", dir.display()))?;
        }
    }
    Ok(())
}

fn fetch_tarball(
    cfg: &Config,
    url: &str,
    archive_name: &str,
    extracted_dir: &std::path::Path,
    force: bool,
) -> Result<()> {
    if already_built(extracted_dir, force) {
        println!("skip fetch: {} already extracted", extracted_dir.display());
        return Ok(());
    }

    let archive_path = cfg.sources_dir().join(archive_name);
    if !archive_path.exists() {
        println!("downloading {url}");
        run(Command::new("wget").args(["-O", archive_path.to_str().unwrap(), url]))?;
    }

    let parent = extracted_dir
        .parent()
        .context("extracted dir has no parent")?;
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

fn fetch_uutils(cfg: &Config, force: bool) -> Result<()> {
    let dir = cfg.uutils_build_dir();
    if already_built(&dir.join(".git"), force) {
        println!("skip fetch: {} already cloned", dir.display());
        return Ok(());
    }

    std::fs::create_dir_all(dir.parent().unwrap())?;
    println!("cloning uutils from {}", cfg.uutils.git_url);
    run(Command::new("git").args([
        "clone",
        &cfg.uutils.git_url,
        dir.to_str().unwrap(),
    ]))?;
    run(Command::new("git")
        .args(["checkout", &cfg.uutils.git_rev])
        .current_dir(&dir))?;

    Ok(())
}
