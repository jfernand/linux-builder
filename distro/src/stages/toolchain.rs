use anyhow::Result;
use builder_core::stages::run;
use std::process::Command;

/// Ensures the host build tools needed to compile the from-scratch
/// userland are present: gcc/make/autotools for the autotools-based
/// projects (bash, util-linux, shadow-utils), plus meson/ninja/pkg-config
/// for the meson-based ones starting in Phase 2 (seatd, dbus, and later
/// the Wayland stack). `libexpat1-dev` is dbus's one build-time library
/// dependency (XML parsing) — the built dbus-daemon links it dynamically
/// and we copy the host's libexpat.so into the rootfs at assemble-rootfs
/// time, same as every other dynamic dependency from here on. `gperf`
/// (a perfect-hash-function generator) is eudev's one extra build-time
/// tool. Unlike distroless's musl cross-toolchain, this is all native —
/// the host's own gcc/glibc.
pub fn build_toolchain() -> Result<()> {
    if have("gcc") && have("make") && have("meson") && have("ninja") && have("pkg-config") && have("gperf") {
        println!("build tools already installed");
        return Ok(());
    }

    println!("installing build tools via apt (requires sudo)");
    run(Command::new("sudo").args(["apt-get", "update"]))?;
    run(Command::new("sudo").args([
        "apt-get",
        "install",
        "-y",
        "build-essential",
        "meson",
        "ninja-build",
        "pkg-config",
        "libexpat1-dev",
        "gperf",
    ]))?;
    Ok(())
}

fn have(program: &str) -> bool {
    Command::new("which")
        .arg(program)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

