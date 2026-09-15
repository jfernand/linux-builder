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
//! Each package's `[section]` table comes straight out of the already-
//! loaded `DistroConfig::packages`, not a second parse of the config
//! file.

use anyhow::{Context, Result};
use buildpack_core::config::DistroConfig;
use buildpack_core::{BuildCtx, Buildpack, InstallMode};
use buildpacks::bash::Bash;
use buildpacks::cairo::Cairo;
use buildpacks::cosmic_comp::CosmicComp;
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
use std::path::{Path, PathBuf};

fn configured<T: Buildpack + Default>(cfg: &DistroConfig, key: &str) -> Result<T> {
    let mut bp = T::default();
    bp.configure(&cfg.package_table(key)).with_context(|| format!("configuring buildpack [{key}]"))?;
    Ok(bp)
}

fn jobs() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
}

/// The generic `BuildCtx` for packages with no old-pipeline equivalent
/// (the cairo/weston chain): sources extract flat under
/// `build_dir/sources`, matching `DistroConfig::sources_dir()`.
fn generic_ctx(cfg: &DistroConfig) -> BuildCtx {
    BuildCtx {
        sources_dir: cfg.sources_dir(),
        build_dir: cfg.build_dir.clone(),
        sysroot_dir: cfg.sysroot_dir(),
        rootfs_dir: cfg.rootfs_dir(),
        arch: cfg.image.arch.clone(),
        networking: cfg.networking,
        jobs: jobs(),
        image: cfg.image.clone(),
        kernel_bzimage: PathBuf::new(), // only `pipeline_ctx` below fills this in
    }
}

/// The `BuildCtx` `PipelineStage` impls (`make-image`/`test-qemu`/
/// `write-usb`) run against — `generic_ctx` plus the one extra thing they
/// need that no buildpack does: the kernel's own bzImage output path.
pub fn pipeline_ctx(cfg: &DistroConfig) -> Result<BuildCtx> {
    let kernel = kernel_buildpack(cfg)?;
    let kctx = kernel_ctx(cfg);
    let bzimage = kernel.outputs(&kctx).into_iter().next().map(|o| o.path).unwrap_or_default();
    Ok(BuildCtx { kernel_bzimage: bzimage, ..generic_ctx(cfg) })
}

/// Points `sources_dir` at the OLD pipeline's per-package build
/// directory, so a cutover recognizes what's already built there instead
/// of rebuilding from scratch.
pub fn ctx_with_sources(cfg: &DistroConfig, subdir: &str) -> BuildCtx {
    BuildCtx { sources_dir: cfg.build_dir.join(subdir), ..generic_ctx(cfg) }
}

/// Every cutover package's old on-disk subdirectory name, keyed by
/// buildpack id — `None` means "use the generic shared `sources/` dir"
/// (the 8 cairo/weston-chain packages, plus `distro_init`, which is
/// `Source::InTree` and never reads `sources_dir` at all).
pub fn ctx_for(id: &str, cfg: &DistroConfig) -> BuildCtx {
    let subdir = match id {
        "kernel" => "kernel",
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

pub fn kernel_buildpack(cfg: &DistroConfig) -> Result<Kernel> {
    configured(cfg, "kernel")
}

pub fn kernel_ctx(cfg: &DistroConfig) -> BuildCtx {
    ctx_with_sources(cfg, "kernel")
}

/// Every package `distro` builds, kernel included — it's a real
/// `Buildpack` like everything else here, just one with its own dedicated
/// `build-kernel`/`menu-config`/`list-features` CLI commands (see
/// `kernel_buildpack`/`kernel_ctx` above) rather than running through this
/// list's generic fetch/build. Callers that need to skip it for that
/// CLI-staging reason (kernel builds first and separately, since it's far
/// too expensive to redo needlessly) filter it out themselves below; the
/// dependency graph and rootfs install pass use the full list as-is. In
/// registration order (irrelevant — `topo_order` sorts it for real).
pub fn all_packages(cfg: &DistroConfig) -> Result<Vec<Box<dyn Buildpack>>> {
    Ok(vec![
        Box::new(configured::<Kernel>(cfg, "kernel")?),
        Box::new(configured::<Uutils>(cfg, "uutils")?),
        Box::new(configured::<Bash>(cfg, "bash")?),
        Box::new(configured::<UtilLinux>(cfg, "util_linux")?),
        Box::new(configured::<Shadow>(cfg, "shadow")?),
        Box::new(configured::<Seatd>(cfg, "seatd")?),
        Box::new(configured::<Dbus>(cfg, "dbus")?),
        Box::new(configured::<Eudev>(cfg, "eudev")?),
        Box::new(configured::<Wayland>(cfg, "wayland")?),
        Box::new(configured::<WaylandProtocols>(cfg, "wayland_protocols")?),
        Box::new(configured::<Libxkbcommon>(cfg, "libxkbcommon")?),
        Box::new(configured::<Pixman>(cfg, "pixman")?),
        Box::new(configured::<LibdisplayInfo>(cfg, "libdisplay_info")?),
        Box::new(configured::<Libevdev>(cfg, "libevdev")?),
        Box::new(configured::<Libinput>(cfg, "libinput")?),
        Box::new(configured::<Libdrm>(cfg, "libdrm")?),
        Box::new(configured::<Mesa>(cfg, "mesa")?),
        Box::new(configured::<Zlib>(cfg, "zlib")?),
        Box::new(configured::<Expat>(cfg, "expat")?),
        Box::new(configured::<Libpng>(cfg, "libpng")?),
        Box::new(configured::<Freetype>(cfg, "freetype")?),
        Box::new(configured::<Fontconfig>(cfg, "fontconfig")?),
        Box::new(configured::<Cairo>(cfg, "cairo")?),
        Box::new(configured::<XkeyboardConfig>(cfg, "xkeyboard_config")?),
        Box::new(configured::<Weston>(cfg, "weston")?),
        Box::new(configured::<CosmicComp>(cfg, "cosmic_comp")?),
        Box::new(DistroInit::new()),
    ])
}

/// Fetches every package but the kernel (which has its own `fetch()` via
/// `kernel_buildpack`/`kernel_ctx`, called separately, before this) — no
/// build step. What `distro fetch` calls.
pub fn fetch_new_packages(cfg: &DistroConfig, force: bool) -> Result<()> {
    let packs = all_packages(cfg)?;
    for pack in packs.iter().filter(|p| p.id() != "kernel") {
        let ctx = ctx_for(pack.id(), cfg);
        pack.fetch(&ctx, force).with_context(|| format!("fetching buildpack {}", pack.id()))?;
    }
    Ok(())
}

/// Fetches and builds every package but the kernel, in dependency order —
/// the kernel build is staged separately (its own `build-kernel` command,
/// always run first) since it's far too expensive to fold into this
/// generic loop's `already_built` check. This is what `distro
/// build-userland` actually calls now.
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
    // package list, kernel included — it's a real Buildpack like
    // everything else here, just one that this pipeline stage doesn't
    // itself fetch/build.
    let svg_path = cfg.build_dir.join("dependency-graph.svg");
    if let Err(e) = buildpack_core::graph::write_svg(&packs, |id| ctx_for(id, cfg), &svg_path) {
        println!("warning: couldn't write dependency graph SVG: {e}");
    } else {
        println!("wrote dependency graph to {}", svg_path.display());
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
pub fn install_static_outputs(cfg: &DistroConfig, root: &Path) -> Result<()> {
    let packs = all_packages(cfg)?;

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

/// `distro`'s `builder_tui::Registry` impl — delegates straight back to
/// this module's own functions.
pub struct DistroRegistry;

impl builder_tui::Registry for DistroRegistry {
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
        "sbin/init"
    }
}
