//! Bridges the `buildpack-core`/`buildpacks` world into `distro`'s own
//! CLI. This is now the *entire* build-userland pipeline — every package
//! `distro` builds other than the kernel (which keeps its own dedicated
//! `build-kernel`/`menu-config`/`list-features` commands, see
//! `kernel_buildpack`/`kernel_ctx` below) is a buildpack, in one
//! `topo_order`-sorted list. The old `distro/src/stages/{fetch,
//! userland}.rs` are gone — every package's fetch+build logic that used
//! to live there now lives in its own `buildpacks/src/<name>.rs`.
//!
//! Each package gets a `BuildCtx` with `sources_dir` pointed at its OLD
//! on-disk per-package build directory (`build_dir/<name>/`, matching
//! what `distro/src/config.rs`'s now-removed `*_build_dir()` methods used
//! to compute), rather than a single shared `build_dir/sources` — this is
//! what let every already-built package be recognized as-is when each
//! one was cut over, instead of triggering a redundant rebuild (a kernel
//! rebuild in particular being far too expensive to redo needlessly). The
//! 8 packages that never had an old-pipeline equivalent at all (the
//! cairo/weston chain) are the one exception: they use the shared
//! `build_dir/sources` directory, since there was never an "old location"
//! for them to match.
//!
//! Reads `distro.toml` a second time as a raw `toml::Value` (not through
//! `Config`, which doesn't model most of these sections) purely to hand
//! each buildpack its own `[section]` table; every other value (paths,
//! jobs) comes from the already-loaded `Config`.

use crate::config::Config;
use anyhow::{Context, Result};
use buildpack_core::{BuildCtx, Buildpack, InstallMode};
use buildpacks::bash::Bash;
use buildpacks::cairo::Cairo;
use buildpacks::dbus::Dbus;
use buildpacks::distro_init::DistroInit;
use buildpacks::eudev::Eudev;
use buildpacks::expat::Expat;
use buildpacks::fontconfig::Fontconfig;
use buildpacks::freetype::Freetype;
use buildpacks::kernel::Kernel;
use buildpacks::libdisplay_info::LibdisplayInfo;
use buildpacks::libdrm::Libdrm;
use buildpacks::libevdev::Libevdev;
use buildpacks::libinput::Libinput;
use buildpacks::libpng::Libpng;
use buildpacks::libxkbcommon::Libxkbcommon;
use buildpacks::mesa::Mesa;
use buildpacks::pixman::Pixman;
use buildpacks::seatd::Seatd;
use buildpacks::shadow::Shadow;
use buildpacks::util_linux::UtilLinux;
use buildpacks::uutils::Uutils;
use buildpacks::wayland::Wayland;
use buildpacks::wayland_protocols::WaylandProtocols;
use buildpacks::weston::Weston;
use buildpacks::xkeyboard_config::XkeyboardConfig;
use buildpacks::zlib::Zlib;
use std::path::Path;

fn configured<T: Buildpack + Default>(root: &toml::Value, key: &str) -> Result<T> {
    let mut bp = T::default();
    let empty = toml::Value::Table(Default::default());
    let table = root.get(key).unwrap_or(&empty);
    bp.configure(table).with_context(|| format!("configuring buildpack [{key}]"))?;
    Ok(bp)
}

fn load_table(config_path: &Path) -> Result<toml::Value> {
    let text = std::fs::read_to_string(config_path)
        .with_context(|| format!("reading {}", config_path.display()))?;
    toml::from_str(&text).with_context(|| format!("parsing {}", config_path.display()))
}

fn jobs() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
}

/// The generic `BuildCtx` for packages with no old-pipeline equivalent
/// (the cairo/weston chain): sources extract flat under
/// `build_dir/sources`, matching `Config::sources_dir()`.
fn generic_ctx(cfg: &Config) -> BuildCtx {
    BuildCtx {
        sources_dir: cfg.sources_dir(),
        build_dir: cfg.build_dir.clone(),
        sysroot_dir: cfg.sysroot_dir(),
        rootfs_dir: cfg.rootfs_dir(),
        arch: cfg.image.arch.clone(),
        networking: cfg.networking,
        jobs: jobs(),
    }
}

/// Points `sources_dir` at the OLD pipeline's per-package build
/// directory, so a cutover recognizes what's already built there instead
/// of rebuilding from scratch.
pub fn ctx_with_sources(cfg: &Config, subdir: &str) -> BuildCtx {
    BuildCtx { sources_dir: cfg.build_dir.join(subdir), ..generic_ctx(cfg) }
}

/// Every cutover package's old on-disk subdirectory name, keyed by
/// buildpack id — `None` means "use the generic shared `sources/` dir"
/// (the 8 cairo/weston-chain packages, plus `distro_init`, which is
/// `Source::InTree` and never reads `sources_dir` at all).
fn ctx_for(id: &str, cfg: &Config) -> BuildCtx {
    let subdir = match id {
        "uutils" => return BuildCtx { sources_dir: cfg.build_dir.clone(), ..generic_ctx(cfg) },
        "bash" => "bash",
        "shadow" => "shadow",
        "util_linux" => "util-linux",
        "seatd" => "seatd",
        "dbus" => "dbus",
        "eudev" => "eudev",
        "wayland" => "wayland",
        "wayland_protocols" => "wayland-protocols",
        "libxkbcommon" => "libxkbcommon",
        "pixman" => "pixman",
        "libdisplay_info" => "libdisplay-info",
        "libevdev" => "libevdev",
        "libinput" => "libinput",
        "libdrm" => "libdrm",
        "mesa" => "mesa",
        _ => return generic_ctx(cfg),
    };
    ctx_with_sources(cfg, subdir)
}

pub fn kernel_buildpack(config_path: &Path) -> Result<Kernel> {
    let root = load_table(config_path)?;
    configured(&root, "kernel")
}

pub fn kernel_ctx(cfg: &Config) -> BuildCtx {
    ctx_with_sources(cfg, "kernel")
}

/// The full non-kernel package list, in registration order (irrelevant —
/// `topo_order` sorts it for real).
fn all_packages(root: &toml::Value) -> Result<Vec<Box<dyn Buildpack>>> {
    Ok(vec![
        Box::new(configured::<Uutils>(root, "uutils")?),
        Box::new(configured::<Bash>(root, "bash")?),
        Box::new(configured::<UtilLinux>(root, "util_linux")?),
        Box::new(configured::<Shadow>(root, "shadow")?),
        Box::new(configured::<Seatd>(root, "seatd")?),
        Box::new(configured::<Dbus>(root, "dbus")?),
        Box::new(configured::<Eudev>(root, "eudev")?),
        Box::new(configured::<Wayland>(root, "wayland")?),
        Box::new(configured::<WaylandProtocols>(root, "wayland_protocols")?),
        Box::new(configured::<Libxkbcommon>(root, "libxkbcommon")?),
        Box::new(configured::<Pixman>(root, "pixman")?),
        Box::new(configured::<LibdisplayInfo>(root, "libdisplay_info")?),
        Box::new(configured::<Libevdev>(root, "libevdev")?),
        Box::new(configured::<Libinput>(root, "libinput")?),
        Box::new(configured::<Libdrm>(root, "libdrm")?),
        Box::new(configured::<Mesa>(root, "mesa")?),
        Box::new(configured::<Zlib>(root, "zlib")?),
        Box::new(configured::<Expat>(root, "expat")?),
        Box::new(configured::<Libpng>(root, "libpng")?),
        Box::new(configured::<Freetype>(root, "freetype")?),
        Box::new(configured::<Fontconfig>(root, "fontconfig")?),
        Box::new(configured::<Cairo>(root, "cairo")?),
        Box::new(configured::<XkeyboardConfig>(root, "xkeyboard_config")?),
        Box::new(configured::<Weston>(root, "weston")?),
        Box::new(DistroInit::new()),
    ])
}

/// Fetches every package but the kernel (which has its own `fetch()` via
/// `kernel_buildpack`/`kernel_ctx`, called separately) — no build step.
/// What `distro fetch` calls.
pub fn fetch_new_packages(config_path: &Path, cfg: &Config, force: bool) -> Result<()> {
    let root = load_table(config_path)?;
    let packs = all_packages(&root)?;
    for pack in &packs {
        let ctx = ctx_for(pack.id(), cfg);
        pack.fetch(&ctx, force).with_context(|| format!("fetching buildpack {}", pack.id()))?;
    }
    Ok(())
}

/// Fetches and builds every package but the kernel, in dependency order.
/// This is what `distro build-userland` actually calls now.
pub fn build_new_packages(config_path: &Path, cfg: &Config, force: bool) -> Result<()> {
    let root = load_table(config_path)?;
    let packs = all_packages(&root)?;
    let order = buildpack_core::graph::topo_order(&packs)?;

    let svg_path = cfg.build_dir.join("dependency-graph.svg");
    if let Err(e) = buildpack_core::graph::write_svg(&packs, |id| ctx_for(id, cfg), &svg_path) {
        println!("warning: couldn't write dependency graph SVG: {e}");
    } else {
        println!("wrote dependency graph to {}", svg_path.display());
    }

    for &i in &order {
        let pack = &packs[i];
        let ctx = ctx_for(pack.id(), cfg);
        pack.fetch(&ctx, force).with_context(|| format!("fetching buildpack {}", pack.id()))?;
        pack.build(&ctx, force).with_context(|| format!("building buildpack {}", pack.id()))?;
    }

    Ok(())
}

/// Copies every `StaticArtifacts` package's declared outputs (and their
/// symlinks) into the rootfs — the generic replacement for
/// `rootfs.rs`'s old per-package `install_coreutils`/`install_bash`/
/// `install_util_linux`/`install_shadow`/`install_init` functions.
/// `Sysroot`-mode packages need no per-package install step at all:
/// `install_sysroot`'s bulk `cp -a` already picks up whatever any of them
/// left in the shared sysroot.
pub fn install_static_outputs(config_path: &Path, cfg: &Config, root: &Path) -> Result<()> {
    let root_toml = load_table(config_path)?;
    let packs = all_packages(&root_toml)?;

    for pack in &packs {
        if !matches!(pack.install_mode(), InstallMode::StaticArtifacts) {
            continue;
        }
        let ctx = ctx_for(pack.id(), cfg);
        for out in pack.outputs(&ctx) {
            buildpack_core::install::install_output(root, &out)?;
        }
    }

    Ok(())
}
