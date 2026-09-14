//! Bridges the `buildpack-core`/`buildpacks` world into `distroless`'s
//! own CLI. Mirrors `distro`'s `stages/buildpacks.rs`, scaled down to
//! `distroless`'s 3 packages: `kernel` (reused unchanged — the same
//! `buildpacks::kernel::Kernel` `distro` uses; kernel builds have no
//! musl/glibc-specific behavior at all), `busybox`, and `uutils` (the
//! `Musl` variant, per `buildpacks::uutils::UutilsVariant`).
//!
//! Each package's `BuildCtx` points `sources_dir` at its OLD on-disk
//! per-package build directory (matching what
//! `builder_core::config::Config`'s `kernel_build_dir()`/
//! `busybox_build_dir()`/`uutils_build_dir()` used to compute), so
//! already-built trees are recognized as-is rather than triggering a
//! redundant rebuild.

use anyhow::{Context, Result};
use builder_core::config::Config;
use buildpack_core::{BuildCtx, Buildpack, InstallMode};
use buildpacks::busybox::Busybox;
use buildpacks::kernel::Kernel;
use buildpacks::uutils::Uutils;
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

/// `sources_dir` defaults to `build_dir` itself (matching uutils' old
/// `uutils_build_dir()`, which has no version-numbered subdirectory) —
/// `ctx_for` overrides it per-package below.
fn base_ctx(cfg: &Config) -> BuildCtx {
    BuildCtx {
        sources_dir: cfg.build_dir.clone(),
        build_dir: cfg.build_dir.clone(),
        sysroot_dir: cfg.build_dir.join("sysroot"), // unused: distroless has no sysroot packages
        rootfs_dir: cfg.rootfs_dir(),
        arch: cfg.image.arch.clone(),
        networking: cfg.networking,
        jobs: jobs(),
    }
}

fn ctx_with_sources(cfg: &Config, subdir: &str) -> BuildCtx {
    BuildCtx { sources_dir: cfg.build_dir.join(subdir), ..base_ctx(cfg) }
}

pub fn kernel_buildpack(config_path: &Path) -> Result<Kernel> {
    let root = load_table(config_path)?;
    configured(&root, "kernel")
}

pub fn kernel_ctx(cfg: &Config) -> BuildCtx {
    ctx_with_sources(cfg, "kernel")
}

/// Every package `distroless` builds, kernel included — it's a real
/// `Buildpack` like uutils/busybox, just one with its own dedicated
/// `build-kernel`/`menu-config`/`list-features` CLI commands (see
/// `kernel_buildpack`/`kernel_ctx` above), so fetch/build below skip it
/// by id rather than it being left out of this list entirely.
fn all_packages(root: &toml::Value) -> Result<Vec<Box<dyn Buildpack>>> {
    Ok(vec![
        Box::new(configured::<Kernel>(root, "kernel")?),
        Box::new(configured::<Uutils>(root, "uutils")?.into_musl()),
        Box::new(configured::<Busybox>(root, "busybox")?),
    ])
}

fn ctx_for(id: &str, cfg: &Config) -> BuildCtx {
    match id {
        "kernel" => kernel_ctx(cfg),
        "busybox" => ctx_with_sources(cfg, "busybox"),
        _ => base_ctx(cfg), // uutils: sources_dir = build_dir itself
    }
}

/// Fetches uutils + busybox — no build step (the kernel has its own
/// `fetch()` via `kernel_buildpack`/`kernel_ctx`, called separately,
/// before this). Called by `distroless fetch`.
pub fn fetch_new_packages(config_path: &Path, cfg: &Config, force: bool) -> Result<()> {
    let root = load_table(config_path)?;
    for pack in all_packages(&root)?.iter().filter(|p| p.id() != "kernel") {
        let ctx = ctx_for(pack.id(), cfg);
        pack.fetch(&ctx, force).with_context(|| format!("fetching buildpack {}", pack.id()))?;
    }
    Ok(())
}

/// Fetches and builds uutils + busybox, kernel excluded (staged
/// separately via its own `build-kernel` command, always run first).
/// Called by `distroless build-userland`.
pub fn build_new_packages(config_path: &Path, cfg: &Config, force: bool) -> Result<()> {
    let root = load_table(config_path)?;
    let packs = all_packages(&root)?;
    let order = buildpack_core::graph::topo_order(&packs)?;

    for &i in &order {
        let pack = &packs[i];
        if pack.id() == "kernel" {
            continue;
        }
        let ctx = ctx_for(pack.id(), cfg);
        pack.fetch(&ctx, force).with_context(|| format!("fetching buildpack {}", pack.id()))?;
        pack.build(&ctx, force).with_context(|| format!("building buildpack {}", pack.id()))?;
    }

    // Unlike the fetch/build loop above, the graph is drawn from the full
    // package list, kernel included.
    let svg_path = cfg.build_dir.join("dependency-graph.svg");
    if let Err(e) = buildpack_core::graph::write_svg(&packs, |id| ctx_for(id, cfg), &svg_path) {
        println!("warning: couldn't write dependency graph SVG: {e}");
    } else {
        println!("wrote dependency graph to {}", svg_path.display());
    }
    Ok(())
}

/// Copies every `StaticArtifacts` package's declared outputs (and their
/// symlinks) into the rootfs.
pub fn install_static_outputs(config_path: &Path, cfg: &Config, root: &Path) -> Result<()> {
    let root_toml = load_table(config_path)?;
    for pack in &all_packages(&root_toml)? {
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
