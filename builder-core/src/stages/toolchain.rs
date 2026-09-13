use super::run;
use anyhow::{Context, Result};
use std::process::Command;

const MUSL_TARGET: &str = "x86_64-unknown-linux-musl";

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

pub fn musl_target() -> &'static str {
    MUSL_TARGET
}
