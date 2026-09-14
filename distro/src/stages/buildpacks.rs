//! Bridges the `buildpack-core`/`buildpacks` world into `distro`'s own
//! CLI. Two distinct groups live here:
//!
//! - The cairo/weston chain (zlib, expat, libpng, freetype, fontconfig,
//!   cairo, xkeyboard_config, weston): no old-pipeline equivalent at
//!   all, pure addition, built by `build_new_packages` as part of
//!   `build-userland`.
//! - kernel, util-linux, Mesa: CUT OVER from the old
//!   `distro/src/stages/{kernel,userland}.rs` implementations — the real
//!   `build-kernel`/`menu-config`/`list-features`/`build-userland`
//!   subcommands now call the buildpack versions, and the old duplicate
//!   code has been removed. Each of these three gets its own `BuildCtx`
//!   with `sources_dir` pointed at its OLD on-disk location
//!   (`build_dir/kernel`, `build_dir/util-linux`, `build_dir/mesa`, not
//!   the shared `build_dir/sources` every other buildpack uses) so the
//!   already-built trees already on disk are recognized as-is instead of
//!   the cutover triggering a redundant rebuild — a kernel rebuild in
//!   particular is far too expensive to redo needlessly.
//!
//! Reads `distro.toml` a second time as a raw `toml::Value` (not through
//! `Config`, which doesn't model the 8 new sections) purely to hand each
//! buildpack its own `[section]` table; every other value (paths, jobs)
//! comes from the already-loaded `Config`.

use crate::config::Config;
use anyhow::{Context, Result};
use buildpack_core::{BuildCtx, Buildpack};
use buildpacks::cairo::Cairo;
use buildpacks::expat::Expat;
use buildpacks::fontconfig::Fontconfig;
use buildpacks::freetype::Freetype;
use buildpacks::kernel::Kernel;
use buildpacks::libpng::Libpng;
use buildpacks::mesa::Mesa;
use buildpacks::util_linux::UtilLinux;
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

/// The generic `BuildCtx` most buildpacks use: sources extract flat under
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

/// See the module doc comment: points `sources_dir` at the OLD pipeline's
/// per-package build directory, so a cutover recognizes what's already
/// built there instead of rebuilding from scratch.
pub fn ctx_with_sources(cfg: &Config, subdir: &str) -> BuildCtx {
    BuildCtx { sources_dir: cfg.build_dir.join(subdir), ..generic_ctx(cfg) }
}

pub fn kernel_buildpack(config_path: &Path) -> Result<Kernel> {
    let root = load_table(config_path)?;
    configured(&root, "kernel")
}

pub fn kernel_ctx(cfg: &Config) -> BuildCtx {
    ctx_with_sources(cfg, "kernel")
}

/// Built directly from `cfg.util_linux` (already-typed on `Config`) via a
/// round trip through `toml::Value`, rather than re-reading `distro.toml`
/// — `rootfs.rs` only has `Config`, not the config file path, when
/// assembling the rootfs.
pub fn util_linux_buildpack(cfg: &Config) -> Result<UtilLinux> {
    let mut bp = UtilLinux::default();
    let table = toml::Value::try_from(&cfg.util_linux).context("serializing [util_linux] config")?;
    bp.configure(&table)?;
    Ok(bp)
}

pub fn util_linux_ctx(cfg: &Config) -> BuildCtx {
    ctx_with_sources(cfg, "util-linux")
}

/// Builds the cairo/weston chain plus util-linux and Mesa (cut over from
/// the old pipeline), all in dependency order. Called from
/// `build_userland`, after the old pipeline's own remaining packages
/// (seatd, dbus, eudev, wayland, ... — util-linux and Mesa are no longer
/// among them, removed from `userland.rs`).
pub fn build_new_packages(config_path: &Path, cfg: &Config, force: bool) -> Result<()> {
    let root = load_table(config_path)?;

    let packs: Vec<Box<dyn Buildpack>> = vec![
        Box::new(configured::<UtilLinux>(&root, "util_linux")?),
        Box::new(configured::<Zlib>(&root, "zlib")?),
        Box::new(configured::<Expat>(&root, "expat")?),
        Box::new(configured::<Libpng>(&root, "libpng")?),
        Box::new(configured::<Freetype>(&root, "freetype")?),
        Box::new(configured::<Fontconfig>(&root, "fontconfig")?),
        Box::new(configured::<Mesa>(&root, "mesa")?),
        Box::new(configured::<Cairo>(&root, "cairo")?),
        Box::new(configured::<XkeyboardConfig>(&root, "xkeyboard_config")?),
        Box::new(configured::<Weston>(&root, "weston")?),
    ];

    let order = buildpack_core::graph::topo_order(&packs)?;

    let generic = generic_ctx(cfg);
    let util_linux_ctx = ctx_with_sources(cfg, "util-linux");
    let mesa_ctx = ctx_with_sources(cfg, "mesa");

    for &i in &order {
        let pack = &packs[i];
        let ctx = match pack.id() {
            "util_linux" => &util_linux_ctx,
            "mesa" => &mesa_ctx,
            _ => &generic,
        };
        pack.fetch(ctx, force).with_context(|| format!("fetching buildpack {}", pack.id()))?;
        pack.build(ctx, force).with_context(|| format!("building buildpack {}", pack.id()))?;
    }

    Ok(())
}
