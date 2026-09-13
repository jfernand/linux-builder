use anyhow::Result;
use builder_core::stages::run;
use std::process::Command;

/// Ensures the host build tools needed to compile the from-scratch
/// userland (bash today; util-linux/shadow-utils once that slice of
/// Phase 1 is built) are present. Unlike distroless's musl cross-toolchain,
/// this is all native — the host's own gcc/make/autotools.
pub fn build_toolchain() -> Result<()> {
    if have("gcc") && have("make") {
        println!("build tools already installed");
        return Ok(());
    }

    println!("installing build-essential via apt (requires sudo)");
    run(Command::new("sudo").args(["apt-get", "update"]))?;
    run(Command::new("sudo").args(["apt-get", "install", "-y", "build-essential"]))?;
    Ok(())
}

fn have(program: &str) -> bool {
    Command::new("which")
        .arg(program)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

