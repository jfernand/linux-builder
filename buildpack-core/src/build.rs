//! Shared build-system helpers, ported near-verbatim from
//! `distro/src/stages/userland.rs`'s `meson_build_and_install`/
//! `autotools_build_and_install`/`sysroot_env`. Deliberately plain
//! functions, not a `BuildSystem` sub-trait — see the plan file's "No
//! BuildSystem sub-trait" note for why.

use crate::run::{already_built, run_in};
use crate::{Buildpack, BuildCtx, Source};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn num_cpus() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
}

fn sysroot_abs(ctx: &BuildCtx) -> Result<PathBuf> {
    Ok(std::env::current_dir().context("getting current directory")?.join(&ctx.sysroot_dir))
}

/// Points `PKG_CONFIG_PATH`/`PKG_CONFIG_SYSROOT_DIR` at the shared sysroot
/// so one package's build can find an earlier one, forces the system
/// pkg-config (dodges a Homebrew-pkgconfig-leak bug class hit repeatedly
/// during Phase 2/3), and prepends the sysroot's `bin`/`sbin` (plus
/// `~/.local/bin`, for Mesa's pip-installed newer meson) to `PATH`.
/// Applied to every meson/configure/ninja/make invocation.
///
/// Deliberately does NOT set `PKG_CONFIG_LIBDIR` to exclude pkg-config's
/// host default search path — that was tried, and it broke a real,
/// legitimate case: `wayland-server` (already built by the old pipeline)
/// needs `libffi`, which — like glibc itself — this project deliberately
/// never builds from source, relying on the host's copy instead
/// (`HOST_DYNAMIC_LIBS` already copies `libffi.so.8` for the same reason).
/// Excluding the host search path made that legitimate host dependency
/// unfindable. The tradeoff this leaves in place: an unwanted *optional*
/// host package (pango, glib, ...) can still be "found" via pkg-config's
/// default search and then have its paths incorrectly mangled by
/// `PKG_CONFIG_SYSROOT_DIR` — handled the same way this bug class has
/// been handled every other time it's appeared (Mesa's spirv-tools,
/// libxkbcommon's xkbregistry, weston's own cairo build): an explicit
/// per-package disable once actually hit, not a blanket search restriction.
pub fn sysroot_env(ctx: &BuildCtx, cmd: &mut Command) -> Result<()> {
    let sysroot = sysroot_abs(ctx)?;
    let pkg_config_path = format!(
        "{}:{}",
        sysroot.join("usr/lib/x86_64-linux-gnu/pkgconfig").display(),
        sysroot.join("usr/share/pkgconfig").display(),
    );
    let local_bin = std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".local/bin"))
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
        .env("PKG_CONFIG", "/usr/bin/pkg-config")
        .env("PATH", path);
    Ok(())
}

pub fn meson_build_and_install(ctx: &BuildCtx, dir: &Path, extra_args: &[&str]) -> Result<()> {
    meson_build_and_install_env(ctx, dir, extra_args, &[])
}

/// Same as `meson_build_and_install`, plus extra environment variables
/// applied on top of `sysroot_env` (so a per-package override can win) —
/// for the rare package that needs one more defensive fix in the same
/// spirit as `sysroot_env`'s own `PKG_CONFIG=/usr/bin/pkg-config`: Mesa's
/// LLVM detection walks `PATH` for `llvm-config`, which this host's own
/// Homebrew install shadows with an incompatible major version ahead of
/// the correct `/usr/bin` one.
pub fn meson_build_and_install_env(
    ctx: &BuildCtx,
    dir: &Path,
    extra_args: &[&str],
    env: &[(&str, &str)],
) -> Result<()> {
    let destdir = sysroot_abs(ctx)?;

    let build_dir = dir.join("build");
    if build_dir.exists() {
        std::fs::remove_dir_all(&build_dir)
            .with_context(|| format!("removing stale build dir {}", build_dir.display()))?;
    }

    let mut setup = Command::new("meson");
    setup.arg("setup").arg("build").arg("--prefix=/usr").args(extra_args);
    sysroot_env(ctx, &mut setup)?;
    for (k, v) in env {
        setup.env(k, v);
    }
    run_in(dir, &mut setup)?;

    let mut build = Command::new("ninja");
    build.arg("-C").arg("build");
    sysroot_env(ctx, &mut build)?;
    for (k, v) in env {
        build.env(k, v);
    }
    run_in(dir, &mut build)?;

    let mut install = Command::new("ninja");
    install.arg("-C").arg("build").arg("install");
    sysroot_env(ctx, &mut install)?;
    for (k, v) in env {
        install.env(k, v);
    }
    install.env("DESTDIR", destdir);
    run_in(dir, &mut install)?;

    Ok(())
}

/// CMake's own configure/build/install-DESTDIR triad, mirroring
/// `meson_build_and_install`'s shape — the Vulkan ecosystem (headers,
/// loader) is the first thing in this workspace to use CMake rather than
/// meson/autotools.
pub fn cmake_build_and_install(ctx: &BuildCtx, dir: &Path, extra_args: &[&str]) -> Result<()> {
    let destdir = sysroot_abs(ctx)?;

    let build_dir = dir.join("build");
    if build_dir.exists() {
        std::fs::remove_dir_all(&build_dir)
            .with_context(|| format!("removing stale build dir {}", build_dir.display()))?;
    }

    let mut configure = Command::new("cmake");
    configure
        .arg("-S")
        .arg(".")
        .arg("-B")
        .arg("build")
        .arg("-DCMAKE_INSTALL_PREFIX=/usr")
        .arg("-DCMAKE_BUILD_TYPE=Release")
        .args(extra_args);
    sysroot_env(ctx, &mut configure)?;
    run_in(dir, &mut configure)?;

    let mut build = Command::new("cmake");
    build.arg("--build").arg("build").arg("--parallel").arg(num_cpus().to_string());
    sysroot_env(ctx, &mut build)?;
    run_in(dir, &mut build)?;

    let mut install = Command::new("cmake");
    install.arg("--install").arg("build");
    sysroot_env(ctx, &mut install)?;
    install.env("DESTDIR", destdir);
    run_in(dir, &mut install)?;

    Ok(())
}

pub fn autotools_build_and_install(ctx: &BuildCtx, dir: &Path, extra_args: &[&str]) -> Result<()> {
    let destdir = sysroot_abs(ctx)?;
    let mut configure = Command::new("sh");
    configure.arg("configure").arg("--prefix=/usr").args(extra_args);
    sysroot_env(ctx, &mut configure)?;
    run_in(dir, &mut configure)?;

    let mut make = Command::new("make");
    make.arg(format!("-j{}", num_cpus()));
    sysroot_env(ctx, &mut make)?;
    run_in(dir, &mut make)?;

    let mut install = Command::new("make");
    install.arg("install");
    sysroot_env(ctx, &mut install)?;
    install.env("DESTDIR", destdir);
    run_in(dir, &mut install)
}

/// `configure`/`make`-based build with no DESTDIR/sysroot install step —
/// for `StaticArtifacts` packages (bash, util-linux, shadow) that just
/// produce a standalone static binary in-place, consumed later by an
/// explicit rootfs copy rather than installed via DESTDIR.
///
/// `configure_env` is set as environment on the `configure` invocation
/// (e.g. bash's plain `LDFLAGS=-static`, which — unlike util-linux/
/// shadow's libtool-mediated `-all-static` — configure-time is fine
/// for). `make_vars` are passed as `make`-time command-line variable
/// assignments instead (e.g. `LDFLAGS=-all-static`): libtool's fully-static
/// flag has to be make-time, not configure-time — configure's own
/// compiler sanity check calls gcc directly, before libtool is set up to
/// translate the flag, so gcc itself rejects it as invalid ("C compiler
/// cannot create executables") if it's set that early.
pub fn autotools_build_static(
    dir: &Path,
    extra_args: &[&str],
    configure_env: &[(&str, &str)],
    make_vars: &[(&str, &str)],
) -> Result<()> {
    let mut configure = Command::new("sh");
    configure.arg("configure").args(extra_args);
    for (k, v) in configure_env {
        configure.env(k, v);
    }
    run_in(dir, &mut configure)?;

    let mut make = Command::new("make");
    make.arg(format!("-j{}", num_cpus()));
    for (k, v) in make_vars {
        make.arg(format!("{k}={v}"));
    }
    run_in(dir, &mut make)
}

/// Cargo's actual output directory for a build run from `source_dir` —
/// normally `source_dir/target`, but cargo honors `CARGO_TARGET_DIR` when
/// set (e.g. to a shared build cache outside the repo), which overrides
/// that per-project default entirely.
pub fn cargo_target_dir(source_dir: &Path) -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| source_dir.join("target"))
}

pub fn cargo_build_release(dir: &Path, target: &str, extra_args: &[&str], env: &[(&str, &str)]) -> Result<()> {
    let mut cmd = Command::new("cargo");
    cmd.args(["build", "--release", "--target", target]).args(extra_args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    run_in(dir, &mut cmd)
}

/// Native `cargo build --release`, with the sysroot's `PKG_CONFIG_PATH`/
/// `PKG_CONFIG_SYSROOT_DIR` applied the same way `meson_build_and_install`/
/// `autotools_build_and_install` do — for Rust packages whose build.rs or
/// `*-sys` crates use `pkg-config` to link against sysroot C libraries
/// (unlike `uutils`, a pure-Rust cross-target build with no C
/// dependencies at all, or `distro-init`, in-tree with no sysroot
/// dependencies). No explicit `--target`: these are native builds.
pub fn cargo_build_release_sysroot(
    ctx: &BuildCtx,
    dir: &Path,
    extra_args: &[&str],
    env: &[(&str, &str)],
) -> Result<()> {
    let mut cmd = Command::new("cargo");
    cmd.arg("build").arg("--release").args(extra_args);
    sysroot_env(ctx, &mut cmd)?;
    for (k, v) in env {
        cmd.env(k, v);
    }
    run_in(dir, &mut cmd)
}

/// Generic tarball/git fetch+extract+patch, used as `Buildpack::fetch`'s
/// default implementation.
pub fn default_fetch<T: Buildpack + ?Sized>(bp: &T, ctx: &BuildCtx, force: bool) -> Result<()> {
    std::fs::create_dir_all(&ctx.sources_dir).context("creating sources dir")?;

    for source in bp.sources(ctx) {
        let extracted_dir = match &source {
            Source::Tarball { archive_name, extracted_dir_name, url } => {
                let extracted_dir = ctx.sources_dir.join(extracted_dir_name);
                if !already_built(&extracted_dir, force) {
                    let archive_abs = ctx.sources_dir.join(archive_name);
                    if !archive_abs.exists() {
                        // Args are relative to sources_dir, since run_in
                        // sets that as the child process's cwd — passing
                        // the already sources_dir-joined path here would
                        // double it up (sources_dir/sources_dir/...).
                        run_in(
                            &ctx.sources_dir,
                            Command::new("wget").arg("-O").arg(archive_name).arg(url),
                        )?;
                    }
                    run_in(&ctx.sources_dir, Command::new("tar").arg("-xf").arg(archive_name))?;
                }
                extracted_dir
            }
            Source::Git { url, rev, checkout_dir_name } => {
                let dir = ctx.sources_dir.join(checkout_dir_name);
                if !already_built(&dir.join(".git"), force) {
                    if !dir.exists() {
                        run_in(
                            &ctx.sources_dir,
                            Command::new("git").arg("clone").arg(url).arg(checkout_dir_name),
                        )?;
                    }
                    run_in(&dir, Command::new("git").arg("checkout").arg(rev))?;
                }
                dir
            }
            Source::InTree => continue,
        };

        for patch in bp.patches(ctx) {
            (patch.apply)(&extracted_dir)
                .with_context(|| format!("applying patch: {}", patch.description))?;
        }
    }

    Ok(())
}
