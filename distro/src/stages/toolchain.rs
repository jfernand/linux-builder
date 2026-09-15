use anyhow::Result;
use buildpack_core::run::run;
use std::process::Command;

/// Mesa (Phase 3) needs a newer meson than Ubuntu 24.04's own `apt` package
/// ships (1.3.2) — `pip install --user` gets a current one into
/// `~/.local/bin`, which `sysroot_env` in `userland.rs` puts ahead of the
/// system one on `PATH` for every build invocation.
const MIN_MESON_VERSION: (u32, u32) = (1, 4);

/// Ensures the host build tools needed to compile the from-scratch
/// userland are present: gcc/make/autotools for the autotools-based
/// projects (bash, util-linux, shadow-utils), plus meson/ninja/pkg-config
/// for the meson-based ones starting in Phase 2 (seatd, dbus, and later
/// the Wayland stack). `libexpat1-dev` is dbus's one build-time library
/// dependency (XML parsing) — the built dbus-daemon links it dynamically
/// and we copy the host's libexpat.so into the rootfs at assemble-rootfs
/// time, same as every other dynamic dependency from here on. `gperf`
/// (a perfect-hash-function generator) is eudev's one extra build-time
/// tool. `glslang-tools` provides `glslangValidator`, Mesa's build-time
/// GLSL→SPIR-V compiler for its `swrast` (lavapipe) Vulkan driver.
/// `libpolly-18-dev` is a separate apt package from `llvm-18-dev` itself
/// — Ubuntu's `llvm-config` reports Polly/PollyISL as available link
/// components regardless, so linking against static LLVM (Mesa's
/// `shared-llvm=disabled`, avoiding a libLLVM runtime dependency on
/// target) fails at link time without it, even though nothing here uses
/// Polly's actual functionality. All build tools, none of them end up on
/// target, so they belong here alongside meson/ninja, not as from-source
/// buildpacks. Unlike distroless's musl cross-toolchain, this is all
/// native — the host's own gcc/glibc.
pub fn build_toolchain() -> Result<()> {
    if !(have("gcc")
        && have("make")
        && have("meson")
        && have("ninja")
        && have("pkg-config")
        && have("gperf")
        && have("glslangValidator")
        && Command::new("sh")
            .arg("-c")
            .arg("dpkg -s libpolly-18-dev >/dev/null 2>&1")
            .status()
            .map(|s| s.success())
            .unwrap_or(false))
    {
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
            "python3-pip",
            "glslang-tools",
            "libpolly-18-dev",
        ]))?;
    }

    if meson_version() < MIN_MESON_VERSION {
        println!("apt's meson is older than {MIN_MESON_VERSION:?}; installing a newer one via pip --user");
        run(Command::new("python3").args([
            "-m",
            "pip",
            "install",
            "--user",
            "--break-system-packages",
            "--upgrade",
            "meson",
        ]))?;
    }

    Ok(())
}

/// Parses `meson --version`'s "1.3.2"-style output into (major, minor),
/// ignoring the patch component (meson's own versioning doesn't need it
/// for this comparison). Missing/unparseable reads as (0, 0), i.e. "too old".
/// Checks `~/.local/bin/meson` directly (not just whatever "meson" resolves
/// to on this process's own inherited `PATH`) since that's the one
/// `pip install --user` would have produced on a previous run, and the one
/// `userland.rs`'s `sysroot_env` actually prefers at build time.
fn meson_version() -> (u32, u32) {
    let local = std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/bin/meson"));
    let candidate = local.filter(|p| p.exists()).unwrap_or_else(|| "meson".into());

    let Ok(output) = Command::new(candidate).arg("--version").output() else {
        return (0, 0);
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut parts = text.trim().split('.');
    let major = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    (major, minor)
}

fn have(program: &str) -> bool {
    Command::new("which")
        .arg(program)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

