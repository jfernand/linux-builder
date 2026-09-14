use crate::config::Config;
use anyhow::{Context, Result};
use builder_core::stages::{already_built, run_in};
use std::process::Command;

/// Native x86_64-unknown-linux-gnu target — this is the *host's* own
/// default target (unlike distroless's musl cross-compile), so nothing
/// beyond the toolchain already on the machine is needed.
const GNU_TARGET: &str = "x86_64-unknown-linux-gnu";

/// Cargo's actual output directory for a build run from `source_dir` —
/// normally `source_dir/target`, but cargo honors `CARGO_TARGET_DIR` when
/// set (e.g. to a shared build cache outside the repo), which overrides
/// that per-project default entirely.
fn cargo_target_dir(source_dir: &std::path::Path) -> std::path::PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| source_dir.join("target"))
}

pub fn build_userland(cfg: &Config, force: bool) -> Result<()> {
    build_uutils(cfg, force)?;
    build_bash(cfg, force)?;
    build_util_linux(cfg, force)?;
    build_shadow(cfg, force)?;
    build_seatd(cfg, force)?;
    build_dbus(cfg, force)?;
    build_init(force)?;
    Ok(())
}

fn build_uutils(cfg: &Config, force: bool) -> Result<()> {
    let dir = cfg.uutils_build_dir();
    let binary = cargo_target_dir(&dir).join(GNU_TARGET).join("release").join("coreutils");

    if already_built(&binary, force) {
        println!("skip build-uutils: {} already exists", binary.display());
        return Ok(());
    }

    println!("building uutils/coreutils for {GNU_TARGET} (static)");
    run_in(
        &dir,
        Command::new("cargo")
            .args([
                "build",
                "--release",
                "--target",
                GNU_TARGET,
                "--no-default-features",
                "--features",
                // feat_os_unix_musl's full feature set, minus `stty`
                // (pulled in by both feat_Tier1 and feat_require_unix_core)
                // and `stdbuf` (pulled in by feat_require_unix, which is
                // why feat_require_unix_musl is used for the rest instead
                // despite its name — it isn't actually musl-specific,
                // it's feat_require_unix minus stdbuf). Both exclusions
                // are for the same reason: neither builds under plain
                // static linking. `stdbuf` needs a cdylib. `stty` calls
                // cfsetispeed/cfsetospeed, and rustc's bundled lld can't
                // resolve those versioned glibc symbols against a
                // statically-linked binary (reproduces with the system
                // `bfd` linker too, so it's a real static-glibc
                // limitation, not a linker choice).
                "feat_common_core,arch,kill,hostname,hostid,nohup,nproc,sync,timeout,uname,\
                 uptime,whoami,\
                 chgrp,chmod,chown,chroot,groups,id,install,logname,mkfifo,mknod,stat,\
                 pinky,users,who",
            ])
            .env("RUSTFLAGS", "-C target-feature=+crt-static"),
    )?;

    Ok(())
}

fn build_bash(cfg: &Config, force: bool) -> Result<()> {
    let dir = cfg.bash_build_dir();
    let binary = dir.join("bash");

    if already_built(&binary, force) {
        println!("skip build-bash: {} already exists", binary.display());
        return Ok(());
    }

    println!("configuring bash (static) in {}", dir.display());
    run_in(
        &dir,
        Command::new("sh")
            .arg("configure")
            .args(["--without-bash-malloc"])
            .env("LDFLAGS", "-static"),
    )?;

    println!("building bash");
    run_in(&dir, Command::new("make").arg(format!("-j{}", num_cpus())))?;

    Ok(())
}

fn build_util_linux(cfg: &Config, force: bool) -> Result<()> {
    let dir = cfg.util_linux_build_dir();
    let binary = dir.join("agetty");

    if already_built(&binary, force) {
        println!("skip build-util-linux: {} already exists", binary.display());
        return Ok(());
    }

    println!("configuring util-linux (static, agetty+mount only) in {}", dir.display());
    run_in(
        &dir,
        Command::new("sh").arg("configure").args([
            "--disable-all-programs",
            "--enable-agetty",
            "--enable-mount",
            "--enable-libmount",
            "--enable-libblkid",
            "--enable-libuuid",
            "--disable-shared",
            "--enable-static",
        ]),
    )?;

    println!("building util-linux");
    run_in(
        &dir,
        // -all-static (libtool's "genuinely fully static executable" flag,
        // unlike plain -static) has to be a `make`-time LDFLAGS, not a
        // configure-time one: configure's own compiler sanity check calls
        // gcc directly, before libtool is set up to translate the flag, so
        // gcc itself rejects it as invalid ("C compiler cannot create
        // executables") if it's set that early.
        Command::new("make").arg(format!("-j{}", num_cpus())).arg("LDFLAGS=-all-static"),
    )?;

    Ok(())
}

fn build_shadow(cfg: &Config, force: bool) -> Result<()> {
    let dir = cfg.shadow_build_dir();
    let binary = dir.join("src").join("login");

    if already_built(&binary, force) {
        println!("skip build-shadow: {} already exists", binary.display());
        return Ok(());
    }

    println!("configuring shadow-utils (static, no PAM/SELinux) in {}", dir.display());
    run_in(
        &dir,
        Command::new("sh").arg("configure").args([
            "--without-libpam",
            "--without-selinux",
            "--without-acl",
            "--without-attr",
            "--without-audit",
            "--disable-nls",
            "--disable-account-tools-setuid",
            "--disable-shared",
            "--enable-static",
        ]),
    )?;

    println!("building shadow-utils");
    run_in(
        &dir,
        // See build_util_linux: -all-static must be make-time, not
        // configure-time.
        Command::new("make").arg(format!("-j{}", num_cpus())).arg("LDFLAGS=-all-static"),
    )?;

    Ok(())
}

/// seatd is the first thing built here with meson/ninja instead of
/// autotools — and, per its own README, "Depends only on libc," so it
/// could in principle still be statically linked. It's built dynamically
/// anyway for consistency with dbus and everything after it (Phase 2's
/// switch away from Phase 1's all-static approach).
fn build_seatd(cfg: &Config, force: bool) -> Result<()> {
    let dir = cfg.seatd_build_dir();
    let build_dir = dir.join("build");
    let binary = build_dir.join("seatd");

    if already_built(&binary, force) {
        println!("skip build-seatd: {} already exists", binary.display());
        return Ok(());
    }

    println!("configuring seatd in {}", dir.display());
    run_in(
        &dir,
        Command::new("meson").arg("setup").arg("build").args([
            "--prefix=/usr",
            "-Dlibseat-logind=disabled",
            "-Dlibseat-seatd=enabled",
            "-Dserver=enabled",
            "-Dman-pages=disabled",
            "-Dexamples=disabled",
        ]),
    )?;

    println!("building seatd");
    run_in(&dir, Command::new("ninja").arg("-C").arg("build"))?;

    Ok(())
}

/// dbus is the first genuinely dynamically-linked dependency in the
/// distro: it needs libexpat for XML parsing, which isn't practical to
/// statically link (see the config.rs/rootfs.rs comments on the dynamic
/// linker/dependency-copying machinery this introduces).
fn build_dbus(cfg: &Config, force: bool) -> Result<()> {
    let dir = cfg.dbus_build_dir();
    let build_dir = dir.join("build");
    let binary = build_dir.join("bus").join("dbus-daemon");

    if already_built(&binary, force) {
        println!("skip build-dbus: {} already exists", binary.display());
        return Ok(());
    }

    println!("configuring dbus in {}", dir.display());
    run_in(
        &dir,
        Command::new("meson").arg("setup").arg("build").args([
            "--prefix=/usr",
            // Our rootfs has no "messagebus" user (or any non-root user
            // yet) for the daemon to drop privileges to, so it runs and
            // stays as root; /run over the default /var/local/run so the
            // socket ends up where a "standard" system bus expects it.
            "-Druntime_dir=/run",
            "-Dsystem_socket=/run/dbus/system_bus_socket",
            "-Ddbus_user=root",
            "-Dsession_socket_dir=/tmp",
            "-Dsystemd=disabled",
            "-Dselinux=disabled",
            "-Dapparmor=disabled",
            "-Dlaunchd=disabled",
            "-Dx11_autolaunch=disabled",
            "-Ddoxygen_docs=disabled",
            "-Dxml_docs=disabled",
            "-Dqt_help=disabled",
            "-Dmodular_tests=disabled",
            "-Dasserts=false",
        ]),
    )?;

    println!("building dbus");
    run_in(&dir, Command::new("ninja").arg("-C").arg("build"))?;

    Ok(())
}

fn build_init(force: bool) -> Result<()> {
    let binary = init_binary_path();
    if already_built(&binary, force) {
        println!("skip build-init: {} already exists", binary.display());
        return Ok(());
    }

    // Built from the workspace root (distro-init is a workspace member),
    // not from a fetched tarball — it's source we wrote ourselves. Assumes
    // distro is invoked from the repo root, same as distro.toml's own
    // relative paths already do.
    let workspace_root = std::env::current_dir().context("getting current directory")?;

    println!("building distro-init (static)");
    run_in(
        &workspace_root,
        Command::new("cargo")
            .args(["build", "--release", "-p", "distro-init", "--target", GNU_TARGET])
            .env("RUSTFLAGS", "-C target-feature=+crt-static"),
    )?;

    Ok(())
}

/// Where `build_init` leaves the compiled binary — a workspace `target/`
/// path, not under `cfg.build_dir` like the fetched sources, since it's
/// built in place from source already in this repo. Honors `CARGO_TARGET_DIR`
/// (cargo itself does, so a plain `"target"` guess breaks whenever that's
/// set, e.g. to a shared build cache outside the repo).
pub fn init_binary_path() -> std::path::PathBuf {
    cargo_target_dir(&std::path::PathBuf::from(".")).join(GNU_TARGET).join("release").join("distro-init")
}

pub fn uutils_binary_path(cfg: &Config) -> std::path::PathBuf {
    cargo_target_dir(&cfg.uutils_build_dir()).join(GNU_TARGET).join("release").join("coreutils")
}

pub fn agetty_binary_path(cfg: &Config) -> std::path::PathBuf {
    cfg.util_linux_build_dir().join("agetty")
}

pub fn mount_binary_path(cfg: &Config) -> std::path::PathBuf {
    cfg.util_linux_build_dir().join("mount")
}

pub fn umount_binary_path(cfg: &Config) -> std::path::PathBuf {
    cfg.util_linux_build_dir().join("umount")
}

pub fn shadow_binary_path(cfg: &Config, name: &str) -> std::path::PathBuf {
    cfg.shadow_build_dir().join("src").join(name)
}

fn num_cpus() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
}
