use crate::config::Config;
use anyhow::{bail, Context, Result};
use builder_core::stages::{already_built, run};
use std::path::Path;
use std::process::Command;

/// Downloads and extracts the kernel, bash, util-linux, and shadow-utils
/// sources, and clones uutils/coreutils — everything `build-userland` needs.
pub fn fetch(cfg: &Config, force: bool) -> Result<()> {
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
        &cfg.bash.url,
        &format!("bash-{}.tar.gz", cfg.bash.version),
        &cfg.bash_build_dir(),
        force,
    )?;

    fetch_tarball(
        cfg,
        &cfg.util_linux.url,
        &format!("util-linux-{}.tar.xz", cfg.util_linux.version),
        &cfg.util_linux_build_dir(),
        force,
    )?;

    fetch_tarball(
        cfg,
        &cfg.shadow.url,
        &format!("shadow-{}.tar.xz", cfg.shadow.version),
        &cfg.shadow_build_dir(),
        force,
    )?;

    fetch_tarball(
        cfg,
        &cfg.seatd.url,
        &format!("seatd-{}.tar.gz", cfg.seatd.version),
        &cfg.seatd_build_dir(),
        force,
    )?;

    fetch_tarball(
        cfg,
        &cfg.dbus.url,
        &format!("dbus-{}.tar.xz", cfg.dbus.version),
        &cfg.dbus_build_dir(),
        force,
    )?;

    fetch_tarball(
        cfg,
        &cfg.eudev.url,
        &format!("eudev-{}.tar.gz", cfg.eudev.version),
        &cfg.eudev_build_dir(),
        force,
    )?;

    fetch_tarball(
        cfg,
        &cfg.wayland.url,
        &format!("wayland-{}.tar.xz", cfg.wayland.version),
        &cfg.wayland_build_dir(),
        force,
    )?;

    fetch_tarball(
        cfg,
        &cfg.wayland_protocols.url,
        &format!("wayland-protocols-{}.tar.xz", cfg.wayland_protocols.version),
        &cfg.wayland_protocols_build_dir(),
        force,
    )?;

    fetch_tarball(
        cfg,
        &cfg.libxkbcommon.url,
        &format!("libxkbcommon-{}.tar.gz", cfg.libxkbcommon.version),
        &cfg.libxkbcommon_build_dir(),
        force,
    )?;

    fetch_tarball(
        cfg,
        &cfg.pixman.url,
        &format!("pixman-{}.tar.gz", cfg.pixman.version),
        &cfg.pixman_build_dir(),
        force,
    )?;

    fetch_tarball(
        cfg,
        &cfg.libdisplay_info.url,
        &format!("libdisplay-info-{}.tar.xz", cfg.libdisplay_info.version),
        &cfg.libdisplay_info_build_dir(),
        force,
    )?;
    patch_libdisplay_info_hwdata(&cfg.libdisplay_info_build_dir())?;

    fetch_tarball(
        cfg,
        &cfg.libevdev.url,
        &format!("libevdev-{}.tar.gz", cfg.libevdev.version),
        &cfg.libevdev_build_dir(),
        force,
    )?;

    fetch_tarball(
        cfg,
        &cfg.libinput.url,
        &format!("libinput-{}.tar.gz", cfg.libinput.version),
        &cfg.libinput_build_dir(),
        force,
    )?;

    fetch_uutils(cfg, force)?;

    Ok(())
}

fn fetch_tarball(
    cfg: &Config,
    url: &str,
    archive_name: &str,
    extracted_dir: &Path,
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

fn fetch_uutils(cfg: &Config, force: bool) -> Result<()> {
    let dir = cfg.uutils_build_dir();
    if !already_built(&dir.join(".git"), force) {
        std::fs::create_dir_all(dir.parent().unwrap())?;
        println!("cloning uutils from {}", cfg.uutils.git_url);
        run(Command::new("git").args(["clone", &cfg.uutils.git_url, dir.to_str().unwrap()]))?;
        run(Command::new("git").args(["checkout", &cfg.uutils.git_rev]).current_dir(&dir))?;
    } else {
        println!("skip fetch: {} already cloned", dir.display());
    }

    // Runs every time (idempotent), not just on a fresh clone, so an
    // already-cloned checkout from before this patch existed still gets it.
    patch_uutils_binary_path(&dir)?;

    Ok(())
}

/// uutils' multi-call dispatch determines which applet argv[0]/a symlink
/// name maps to. On non-musl Linux it distrusts argv[0] and instead reads
/// the kernel's AT_EXECFN auxval (to stop `env -a` from spoofing which
/// AppArmor/SELinux policy applies) — but if AT_EXECFN comes back empty
/// (observed both on this build host and inside our own built kernel; not
/// something specific to either), it uses that empty path as the binary
/// name instead of falling back to argv0, so every applet invocation
/// (`ls`, `cat`, ...) hits "<unknown binary name>" instead of dispatching.
/// Patches in a fallback to argv0 for that one case, leaving the normal
/// hardened behavior intact otherwise.
fn patch_uutils_binary_path(dir: &Path) -> Result<()> {
    let path = dir.join("src/common/validation.rs");
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("reading {}", path.display()))?;

    const ALREADY_PATCHED: &str = "execfn_bytes.is_empty()\n        || execfn_bytes.rsplit";
    if text.contains(ALREADY_PATCHED) {
        println!("uutils binary_path already patched");
        return Ok(());
    }

    const OLD: &str = "    if execfn_bytes.rsplit(|&b| b == b'/').next() == argv0.as_bytes().rsplit(|&b| b == b'/').next()\n        || execfn_bytes.starts_with(b\"/proc/\")";
    const NEW: &str = "    if execfn_bytes.is_empty()\n        || execfn_bytes.rsplit(|&b| b == b'/').next() == argv0.as_bytes().rsplit(|&b| b == b'/').next()\n        || execfn_bytes.starts_with(b\"/proc/\")";

    if !text.contains(OLD) {
        bail!(
            "couldn't find the expected binary_path code in {} to patch \
             (uutils upstream may have changed it) — see patch_uutils_binary_path",
            path.display()
        );
    }

    println!("patching uutils binary_path (AT_EXECFN-empty fallback)");
    std::fs::write(&path, text.replace(OLD, NEW))
        .with_context(|| format!("writing {}", path.display()))
}

/// libdisplay-info's meson.build looks up `hwdata` (the package providing
/// `/usr/share/hwdata/pnp.ids`, a vendor-ID database it embeds into the
/// built library at compile time — a build-time-only need, nothing reads
/// it at target runtime) via `dependency('hwdata', ...).get_variable(...)`.
/// That variable resolves correctly on the *host* (where hwdata is
/// actually installed), but our `PKG_CONFIG_SYSROOT_DIR` — necessary for
/// every package that genuinely does live under our sysroot — rewrites it
/// into a sysroot path hwdata was never installed under, since hwdata is
/// found via pkg-config's own default search, not our sysroot's
/// `PKG_CONFIG_PATH`. Same class of bug as Mesa's spirv-tools and
/// libxkbcommon's libxml2 (see userland.rs's `sysroot_env`), but neither
/// "force the system pkg-config" nor "disable the optional feature" apply
/// here — `hwdata` isn't optional-and-skippable, and the mangled path
/// comes from a `get_variable()` call the same way wayland-scanner's did.
/// Unlike those, libdisplay-info's own meson.build already has an
/// unconditional fallback to the literal, correct host path in its `else`
/// branch — this patch just always takes it.
fn patch_libdisplay_info_hwdata(dir: &Path) -> Result<()> {
    let path = dir.join("meson.build");
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("reading {}", path.display()))?;

    const OLD: &str = "dep_hwdata = dependency('hwdata', required: false, native: true)\nif dep_hwdata.found()\n\thwdata_dir = dep_hwdata.get_variable(pkgconfig: 'pkgdatadir')\n\tpnp_ids = files(hwdata_dir / 'pnp.ids')\nelse\n\tpnp_ids = files('/usr/share/hwdata/pnp.ids')\nendif";
    const NEW: &str = "pnp_ids = files('/usr/share/hwdata/pnp.ids')";

    if !text.contains("dep_hwdata = dependency(") {
        println!("libdisplay-info hwdata lookup already patched");
        return Ok(());
    }

    if !text.contains(OLD) {
        bail!(
            "couldn't find the expected hwdata lookup code in {} to patch \
             (libdisplay-info upstream may have changed it) — see patch_libdisplay_info_hwdata",
            path.display()
        );
    }

    println!("patching libdisplay-info to always use the literal hwdata path");
    std::fs::write(&path, text.replace(OLD, NEW))
        .with_context(|| format!("writing {}", path.display()))
}
