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
    build_eudev(cfg, force)?;
    build_wayland(cfg, force)?;
    build_wayland_protocols(cfg, force)?;
    build_libxkbcommon(cfg, force)?;
    build_pixman(cfg, force)?;
    build_libdisplay_info(cfg, force)?;
    build_libevdev(cfg, force)?;
    build_libinput(cfg, force)?;
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

/// Absolute path to `cfg.sysroot_dir()` itself (not `.../usr`) — used as
/// both `DESTDIR` for a normal staged install and as `PKG_CONFIG_SYSROOT_DIR`
/// (see `sysroot_env`).
fn sysroot_abs(cfg: &Config) -> Result<std::path::PathBuf> {
    Ok(std::env::current_dir().context("getting current directory")?.join(cfg.sysroot_dir()))
}

/// Points `PKG_CONFIG_PATH` at the sysroot's installed `.pc` files so a
/// package's build can find an earlier one — libinput needs eudev's
/// `libudev.pc`, for instance — and sets `PKG_CONFIG_SYSROOT_DIR` so the
/// `-I`/`-L` paths pkg-config reports for those `.pc` files get rewritten
/// from their baked-in `/usr/...` to the sysroot's real, on-disk
/// `<sysroot>/usr/...` (this is pkg-config's own built-in sysroot handling,
/// automatic for `Cflags`/`Libs`; it does *not* extend to arbitrary custom
/// `.pc` variables, which is exactly the problem `build_wayland` below
/// works around separately). `PATH` gets the sysroot's `bin`/`sbin` too,
/// as a general safety net for any `find_program('some-tool')` by bare
/// name. Applied to every meson/configure/ninja/make invocation from here
/// on, not just the ones that need it yet, since which package needs which
/// earlier one only grows from here.
fn sysroot_env(cfg: &Config, cmd: &mut Command) -> Result<()> {
    let sysroot = sysroot_abs(cfg)?;
    let pkg_config_path = format!(
        "{}:{}",
        sysroot.join("usr/lib/x86_64-linux-gnu/pkgconfig").display(),
        sysroot.join("usr/share/pkgconfig").display(),
    );
    // `~/.local/bin` first, explicitly: Mesa needs a newer meson than
    // apt's own package (see toolchain.rs's pip install --user), and this
    // shouldn't depend on the invoking shell already having that directory
    // on PATH ahead of /usr/bin — a fresh/non-interactive shell might not.
    let local_bin = std::env::var_os("HOME")
        .map(|h| std::path::PathBuf::from(h).join(".local/bin"))
        .filter(|p| p.exists())
        .map(|p| format!("{}:", p.display()))
        .unwrap_or_default();
    let path = format!(
        "{local_bin}{}:{}:{}",
        sysroot.join("usr/bin").display(),
        sysroot.join("usr/sbin").display(),
        std::env::var("PATH").unwrap_or_default(),
    );
    cmd.env("PKG_CONFIG_PATH", pkg_config_path)
        .env("PKG_CONFIG_SYSROOT_DIR", &sysroot)
        // The system pkg-config (pkgconf, /usr/bin/pkg-config), not
        // whichever "pkg-config" a PATH search would otherwise turn up.
        // This host also has Homebrew's own pkg-config ahead on PATH,
        // which bakes in Homebrew's own lib/pkgconfig dirs as *compiled-in*
        // default search paths (unlike the system one, whose defaults are
        // the standard /usr/lib/x86_64-linux-gnu/pkgconfig and friends) —
        // so any host package that happens to be installed via Homebrew at
        // its own nonstandard prefix (this bit us twice already: Mesa's
        // optional spirv-tools support, libxkbcommon's optional
        // xkbregistry needing libxml2) gets "found", and then
        // PKG_CONFIG_SYSROOT_DIR mangles its `-I` path into a sysroot
        // location it was never installed under, breaking the compile.
        // Forcing the system pkg-config avoids the whole class of bug —
        // its defaults never point outside a normal Ubuntu install.
        .env("PKG_CONFIG", "/usr/bin/pkg-config")
        .env("PATH", path);
    Ok(())
}

/// `meson setup <build_dir>/build --prefix=/usr <extra_args>`, then
/// `ninja -C <build_dir>/build install` with `DESTDIR` set to the
/// sysroot — configure, build, and install-to-sysroot in one step, since
/// nothing downstream needs the unconfigured or built-but-not-installed
/// states separately. `--prefix=/usr` (the *final* runtime path, not the
/// sysroot's own on-disk location) matters for any package whose binary
/// does its own prefix-derived path lookups at its own runtime — dbus
/// looks for its `system.conf` under the prefix it was built with, and a
/// sysroot-absolute prefix would bake in a build-time-only path that
/// doesn't exist once the binary is copied into the final rootfs (this
/// broke dbus-daemon in exactly this way before `DESTDIR` replaced a
/// direct sysroot prefix here). See `build_wayland` for the one exception.
fn meson_build_and_install(cfg: &Config, dir: &std::path::Path, extra_args: &[&str]) -> Result<()> {
    let destdir = sysroot_abs(cfg)?;

    // A `--force` rebuild starts every meson project fresh: `meson setup`
    // refuses to run again on an already-configured `build/` (worse, if the
    // directory was last configured by an older meson than is now on PATH —
    // exactly what happened switching to a pip-installed meson for Mesa —
    // it fails outright with "Build data file ... references functions or
    // classes that don't exist" instead of just reconfiguring).
    let build_dir = dir.join("build");
    if build_dir.exists() {
        std::fs::remove_dir_all(&build_dir)
            .with_context(|| format!("removing stale build dir {}", build_dir.display()))?;
    }

    let mut setup = Command::new("meson");
    setup.arg("setup").arg("build").arg("--prefix=/usr").args(extra_args);
    sysroot_env(cfg, &mut setup)?;
    run_in(dir, &mut setup)?;

    let mut build = Command::new("ninja");
    build.arg("-C").arg("build");
    sysroot_env(cfg, &mut build)?;
    run_in(dir, &mut build)?;

    let mut install = Command::new("ninja");
    install.arg("-C").arg("build").arg("install");
    sysroot_env(cfg, &mut install)?;
    install.env("DESTDIR", destdir);
    run_in(dir, &mut install)?;

    Ok(())
}

/// seatd is the first thing built here with meson/ninja instead of
/// autotools — and, per its own README, "Depends only on libc," so it
/// could in principle still be statically linked. It's built dynamically
/// anyway for consistency with dbus and everything after it (Phase 2's
/// switch away from Phase 1's all-static approach).
fn build_seatd(cfg: &Config, force: bool) -> Result<()> {
    let dir = cfg.seatd_build_dir();
    let binary = dir.join("build").join("seatd");

    if already_built(&binary, force) {
        println!("skip build-seatd: {} already exists", binary.display());
        return Ok(());
    }

    println!("configuring/building/installing seatd in {}", dir.display());
    meson_build_and_install(
        cfg,
        &dir,
        &[
            "-Dlibseat-logind=disabled",
            "-Dlibseat-seatd=enabled",
            "-Dserver=enabled",
            "-Dman-pages=disabled",
            "-Dexamples=disabled",
        ],
    )
}

/// dbus is the first genuinely dynamically-linked dependency in the
/// distro: it needs libexpat for XML parsing, which isn't practical to
/// statically link (see the config.rs/rootfs.rs comments on the dynamic
/// linker/dependency-copying machinery this introduces).
fn build_dbus(cfg: &Config, force: bool) -> Result<()> {
    let dir = cfg.dbus_build_dir();
    let binary = dir.join("build").join("bus").join("dbus-daemon");

    if already_built(&binary, force) {
        println!("skip build-dbus: {} already exists", binary.display());
        return Ok(());
    }

    println!("configuring/building/installing dbus in {}", dir.display());
    meson_build_and_install(
        cfg,
        &dir,
        &[
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
        ],
    )
}

/// `<config>/configure --prefix=/usr <extra_args>`, then `make -jN` and
/// `make install DESTDIR=<sysroot>` — the autotools equivalent of
/// `meson_build_and_install`, for eudev (and anything else Phase 2+ pulls
/// in that isn't meson). Same `--prefix=/usr`-not-sysroot-absolute
/// reasoning applies: eudev's own `udevd` looks up its rules/hwdb
/// directories relative to the prefix it was configured with.
fn autotools_build_and_install(cfg: &Config, dir: &std::path::Path, extra_args: &[&str]) -> Result<()> {
    let destdir = sysroot_abs(cfg)?;
    let mut configure = Command::new("sh");
    configure.arg("configure").arg("--prefix=/usr").args(extra_args);
    sysroot_env(cfg, &mut configure)?;
    run_in(dir, &mut configure)?;

    let mut make = Command::new("make");
    make.arg(format!("-j{}", num_cpus()));
    sysroot_env(cfg, &mut make)?;
    run_in(dir, &mut make)?;

    let mut install = Command::new("make");
    install.arg("install");
    sysroot_env(cfg, &mut install)?;
    install.env("DESTDIR", destdir);
    run_in(dir, &mut install)
}

/// eudev is a systemd-independent fork of udev (what Alpine/Void/Gentoo
/// use without systemd) — needed for libudev, which libinput hard-depends
/// on. Unlike seatd/dbus it's autotools, not meson. blkid/SELinux/kmod
/// support are all disabled: our rootfs has no libblkid.so or libselinux
/// to link against (util-linux's libblkid was built statically for
/// Phase 1's static tools, not as a shared library), and no loadable
/// kernel modules to manage.
fn build_eudev(cfg: &Config, force: bool) -> Result<()> {
    let dir = cfg.eudev_build_dir();
    let binary = dir.join("src").join("udev").join("udevd");

    if already_built(&binary, force) {
        println!("skip build-eudev: {} already exists", binary.display());
        return Ok(());
    }

    println!("configuring/building/installing eudev in {}", dir.display());
    autotools_build_and_install(
        cfg,
        &dir,
        &[
            "--sysconfdir=/etc",
            "--libdir=/usr/lib/x86_64-linux-gnu",
            "--disable-blkid",
            "--disable-selinux",
            "--disable-kmod",
            "--disable-manpages",
        ],
    )
}

/// Base Wayland: wire protocol libraries (client/server/cursor/egl) and
/// `wayland-scanner`, the code generator every later Wayland-protocol
/// package (wayland-protocols, Mesa, and eventually the compositor)
/// invokes at its own build time.
///
/// `wayland-scanner`'s own path gets baked into wayland's installed `.pc`
/// file as a custom variable (`wayland_scanner=${bindir}/wayland-scanner`),
/// and later packages' builds `get_variable()` that and directly execute
/// whatever path it names — this used to need special-casing wayland's own
/// build (an absolute, on-disk `--prefix`, installed directly instead of
/// via `DESTDIR`) because a bare `pkg-config --variable=` lookup doesn't
/// rewrite custom variables for a sysroot. It turns out meson's own
/// `PkgConfigDependency` is more thorough than raw pkg-config here: with
/// `PKG_CONFIG_SYSROOT_DIR` set (which `sysroot_env` always sets), meson
/// *does* rewrite this variable before treating it as a program path —
/// confirmed by testing wayland built the same `--prefix=/usr` +
/// `DESTDIR` way as everything else, and wayland-protocols' build finding
/// `wayland-scanner` at the correct sysroot-relocated path regardless. No
/// special case needed.
fn build_wayland(cfg: &Config, force: bool) -> Result<()> {
    let dir = cfg.wayland_build_dir();
    let binary = dir.join("build").join("src").join("wayland-scanner");

    if already_built(&binary, force) {
        println!("skip build-wayland: {} already exists", binary.display());
        return Ok(());
    }

    println!("configuring/building/installing wayland in {}", dir.display());
    meson_build_and_install(
        cfg,
        &dir,
        &["-Ddocumentation=false", "-Dtests=false", "-Ddtd_validation=false"],
    )
}

/// The Wayland protocol XML definitions themselves (xdg-shell and
/// friends) — no library, just data + a pkg-config file Phase 3's
/// compositor build reads to find them. Needs `wayland-scanner` (built
/// above) at its own build time to validate/process a couple of them.
fn build_wayland_protocols(cfg: &Config, force: bool) -> Result<()> {
    let dir = cfg.wayland_protocols_build_dir();
    let marker = dir.join("build").join("meson-private").join("wayland-protocols.pc");

    if already_built(&marker, force) {
        println!("skip build-wayland-protocols: {} already exists", marker.display());
        return Ok(());
    }

    println!("configuring/building/installing wayland-protocols in {}", dir.display());
    meson_build_and_install(cfg, &dir, &["-Dtests=false"])
}

/// Keymap compilation (turns e.g. "us, evdev, pc105" into the actual
/// lookup tables a compositor hands to clients). X11 support is disabled
/// since we don't build libxcb (see the fetch.rs/config.rs comment on
/// that decision) — only XWayland compatibility would need it, and that's
/// not planned. `xkb-config-root` is pinned to the FHS-standard path
/// `/usr/share/X11/xkb` rather than whatever the host happens to have
/// installed (meson's default probes the *host's* pkg-config for
/// `xkeyboard-config` and would otherwise bake in a host path that won't
/// exist in the rootfs); the actual xkeyboard-config data package isn't
/// built yet, so real keymap compilation won't work until it is — not
/// needed until Phase 3 has a compositor to test it with.
fn build_libxkbcommon(cfg: &Config, force: bool) -> Result<()> {
    let dir = cfg.libxkbcommon_build_dir();
    let marker = dir.join("build").join("meson-private").join("xkbcommon.pc");

    if already_built(&marker, force) {
        println!("skip build-libxkbcommon: {} already exists", marker.display());
        return Ok(());
    }

    println!("configuring/building/installing libxkbcommon in {}", dir.display());
    meson_build_and_install(
        cfg,
        &dir,
        &[
            "-Denable-x11=false",
            "-Denable-docs=false",
            "-Dxkb-config-root=/usr/share/X11/xkb",
            // libxkbregistry (XDG-style layout enumeration, e.g. for a
            // settings UI) needs libxml2, which we don't build ourselves —
            // not needed for keymap compilation itself, so left out rather
            // than adding a whole extra from-source package for it.
            "-Denable-xkbregistry=false",
        ],
    )
}

/// Software rasterization — used both as Mesa's software fallback path
/// and directly by some compositor code for operations not worth doing on
/// the GPU.
fn build_pixman(cfg: &Config, force: bool) -> Result<()> {
    let dir = cfg.pixman_build_dir();
    let marker = dir.join("build").join("meson-private").join("pixman-1.pc");

    if already_built(&marker, force) {
        println!("skip build-pixman: {} already exists", marker.display());
        return Ok(());
    }

    println!("configuring/building/installing pixman in {}", dir.display());
    meson_build_and_install(
        cfg,
        &dir,
        &["-Dtests=disabled", "-Ddemos=disabled", "-Dgtk=disabled", "-Dlibpng=disabled", "-Dopenmp=disabled"],
    )
}

/// EDID/DisplayID parsing — how a compositor reads a monitor's own
/// description of its supported modes. No build options to speak of.
fn build_libdisplay_info(cfg: &Config, force: bool) -> Result<()> {
    let dir = cfg.libdisplay_info_build_dir();
    let marker = dir.join("build").join("meson-private").join("libdisplay-info.pc");

    if already_built(&marker, force) {
        println!("skip build-libdisplay-info: {} already exists", marker.display());
        return Ok(());
    }

    println!("configuring/building/installing libdisplay-info in {}", dir.display());
    meson_build_and_install(cfg, &dir, &[])
}

/// libinput's mandatory dependency for reading/writing raw evdev input
/// devices.
fn build_libevdev(cfg: &Config, force: bool) -> Result<()> {
    let dir = cfg.libevdev_build_dir();
    let marker = dir.join("build").join("meson-private").join("libevdev.pc");

    if already_built(&marker, force) {
        println!("skip build-libevdev: {} already exists", marker.display());
        return Ok(());
    }

    println!("configuring/building/installing libevdev in {}", dir.display());
    meson_build_and_install(cfg, &dir, &["-Dtests=disabled", "-Ddocumentation=disabled"])
}

/// Turns raw evdev events into the pointer/keyboard/touch/gesture events
/// a compositor actually wants — the last of Phase 2's libraries, and the
/// reason eudev exists in this pipeline at all (libinput hard-depends on
/// libudev). libwacom (tablet identification) and mtdev (legacy
/// multitouch "protocol A" device translation — effectively unused on any
/// hardware built in the last decade) are both deliberately skipped:
/// niche hardware support not worth two more from-source packages for
/// Phase 2's "does the plumbing work" milestone. Lua plugin support is
/// off for the same reason (no lua on the target yet).
fn build_libinput(cfg: &Config, force: bool) -> Result<()> {
    let dir = cfg.libinput_build_dir();
    let marker = dir.join("build").join("meson-private").join("libinput.pc");

    if already_built(&marker, force) {
        println!("skip build-libinput: {} already exists", marker.display());
        return Ok(());
    }

    println!("configuring/building/installing libinput in {}", dir.display());
    meson_build_and_install(
        cfg,
        &dir,
        &[
            "-Dlibwacom=false",
            "-Dmtdev=false",
            "-Ddebug-gui=false",
            "-Dtests=false",
            "-Ddocumentation=false",
            "-Dlua-plugins=disabled",
        ],
    )
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
