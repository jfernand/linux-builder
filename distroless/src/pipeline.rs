//! Bridges the `buildpack-core`/`buildpacks` world into `distroless`'s
//! own CLI. Mirrors `distro`'s `stages/buildpacks.rs`, scaled down to
//! `distroless`'s 3 packages: `kernel` (reused unchanged — the same
//! `buildpacks::kernel::Kernel` `distro` uses; kernel builds have no
//! musl/glibc-specific behavior at all), `busybox`, and `uutils` (the
//! `Musl` variant, per `buildpacks::uutils::UutilsVariant`).
//!
//! Each package's `BuildCtx` points `sources_dir` at its OLD on-disk
//! per-package build directory (matching what `builder_core::config::
//! Config`'s now-unused `kernel_build_dir()`/`busybox_build_dir()`/
//! `uutils_build_dir()` used to compute), so already-built trees are
//! recognized as-is rather than triggering a redundant rebuild.

use anyhow::{Context, Result};
use buildpack_core::config::DistroConfig;
use buildpack_core::{BuildCtx, Buildpack, InstallMode};
use buildpacks::busybox::Busybox;
use buildpacks::kernel::Kernel;
use buildpacks::uutils::Uutils;
use std::path::{Path, PathBuf};

fn configured<T: Buildpack + Default>(cfg: &DistroConfig, key: &str) -> Result<T> {
    let mut bp = T::default();
    bp.configure(&cfg.package_table(key)).with_context(|| format!("configuring buildpack [{key}]"))?;
    Ok(bp)
}

fn jobs() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
}

/// `sources_dir` defaults to `build_dir` itself (matching uutils' old
/// `uutils_build_dir()`, which has no version-numbered subdirectory) —
/// `ctx_for` overrides it per-package below.
fn base_ctx(cfg: &DistroConfig) -> BuildCtx {
    BuildCtx {
        sources_dir: cfg.build_dir.clone(),
        build_dir: cfg.build_dir.clone(),
        sysroot_dir: cfg.build_dir.join("sysroot"), // unused: distroless has no sysroot packages
        rootfs_dir: cfg.rootfs_dir(),
        arch: cfg.image.arch.clone(),
        networking: cfg.networking,
        jobs: jobs(),
        image: cfg.image.clone(),
        kernel_bzimage: PathBuf::new(), // only `pipeline_ctx` below fills this in
    }
}

/// The `BuildCtx` `PipelineStage` impls (`make-image`/`test-qemu`/
/// `write-usb`) run against — `base_ctx` plus the kernel's own bzImage
/// output path, which no buildpack needs but `make_image` does.
pub fn pipeline_ctx(cfg: &DistroConfig) -> Result<BuildCtx> {
    let kernel = kernel_buildpack(cfg)?;
    let kctx = kernel_ctx(cfg);
    let bzimage = kernel.outputs(&kctx).into_iter().next().map(|o| o.path).unwrap_or_default();
    Ok(BuildCtx { kernel_bzimage: bzimage, ..base_ctx(cfg) })
}

fn ctx_with_sources(cfg: &DistroConfig, subdir: &str) -> BuildCtx {
    BuildCtx { sources_dir: cfg.build_dir.join(subdir), ..base_ctx(cfg) }
}

pub fn kernel_buildpack(cfg: &DistroConfig) -> Result<Kernel> {
    configured(cfg, "kernel")
}

pub fn kernel_ctx(cfg: &DistroConfig) -> BuildCtx {
    ctx_with_sources(cfg, "kernel")
}

/// Every package `distroless` builds, kernel included — it's a real
/// `Buildpack` like uutils/busybox, just one with its own dedicated
/// `build-kernel`/`menu-config`/`list-features` CLI commands (see
/// `kernel_buildpack`/`kernel_ctx` above), so fetch/build below skip it
/// by id rather than it being left out of this list entirely.
pub fn all_packages(cfg: &DistroConfig) -> Result<Vec<Box<dyn Buildpack>>> {
    Ok(vec![
        Box::new(configured::<Kernel>(cfg, "kernel")?),
        Box::new(configured::<Uutils>(cfg, "uutils")?.into_musl()),
        Box::new(configured::<Busybox>(cfg, "busybox")?),
    ])
}

pub fn ctx_for(id: &str, cfg: &DistroConfig) -> BuildCtx {
    match id {
        "kernel" => kernel_ctx(cfg),
        "busybox" => ctx_with_sources(cfg, "busybox"),
        _ => base_ctx(cfg), // uutils: sources_dir = build_dir itself
    }
}

/// Fetches uutils + busybox — no build step (the kernel has its own
/// `fetch()` via `kernel_buildpack`/`kernel_ctx`, called separately,
/// before this). Called by `distroless fetch`.
pub fn fetch_new_packages(cfg: &DistroConfig, force: bool) -> Result<()> {
    for pack in all_packages(cfg)?.iter().filter(|p| p.id() != "kernel") {
        let ctx = ctx_for(pack.id(), cfg);
        pack.fetch(&ctx, force).with_context(|| format!("fetching buildpack {}", pack.id()))?;
    }
    Ok(())
}

/// Fetches and builds uutils + busybox, kernel excluded (staged
/// separately via its own `build-kernel` command, always run first).
/// Called by `distroless build-userland`.
pub fn build_new_packages(cfg: &DistroConfig, force: bool) -> Result<()> {
    let packs = all_packages(cfg)?;
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
pub fn install_static_outputs(cfg: &DistroConfig, root: &Path) -> Result<()> {
    for pack in &all_packages(cfg)? {
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

/// `distroless`'s `builder_tui::Registry` impl — delegates straight back
/// to this module's own functions.
pub struct DistrolessRegistry;

impl builder_tui::Registry for DistrolessRegistry {
    fn all_packages(&self, cfg: &DistroConfig) -> Result<Vec<Box<dyn Buildpack>>> {
        all_packages(cfg)
    }

    fn ctx_for(&self, id: &str, cfg: &DistroConfig) -> BuildCtx {
        ctx_for(id, cfg)
    }

    fn kernel_buildpack(&self, cfg: &DistroConfig) -> Result<Kernel> {
        kernel_buildpack(cfg)
    }

    fn kernel_ctx(&self, cfg: &DistroConfig) -> BuildCtx {
        kernel_ctx(cfg)
    }

    fn pipeline_ctx(&self, cfg: &DistroConfig) -> Result<BuildCtx> {
        pipeline_ctx(cfg)
    }

    fn rootfs_ready_marker(&self) -> &'static str {
        "etc/inittab"
    }
}
