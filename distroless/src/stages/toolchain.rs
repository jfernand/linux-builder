//! `distroless`'s own toolchain setup — musl-gcc + the musl cargo target
//! — genuinely different from `distro`'s (native apt gcc/meson/ninja), so
//! not shared via `buildpack-core` the same way `make-image`/`test-qemu`/
//! `write-usb` are. Ported unchanged from `builder-core/src/stages/
//! toolchain.rs`.

use anyhow::{Context, Result};
use buildpack_core::run::run;
use std::process::Command;

pub const MUSL_TARGET: &str = "x86_64-unknown-linux-musl";

pub fn build_toolchain() -> Result<()> {
    ensure_musl_gcc()?;
    ensure_rust_musl_target()?;
    Ok(())
}

fn ensure_musl_gcc() -> Result<()> {
    if Command::new("which")
        .arg("musl-gcc")
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        println!("musl-gcc already installed");
        return Ok(());
    }

    println!("installing musl-tools via apt (requires sudo)");
    run(Command::new("sudo").args(["apt-get", "update"]))?;
    run(Command::new("sudo").args(["apt-get", "install", "-y", "musl-tools"]))?;
    Ok(())
}

fn ensure_rust_musl_target() -> Result<()> {
    let installed = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .context("running rustup target list")?;
    let installed = String::from_utf8_lossy(&installed.stdout);
    if installed.lines().any(|l| l.trim() == MUSL_TARGET) {
        println!("rust target {MUSL_TARGET} already installed");
        return Ok(());
    }

    println!("adding rust target {MUSL_TARGET}");
    run(Command::new("rustup").args(["target", "add", MUSL_TARGET]))?;
    Ok(())
}
