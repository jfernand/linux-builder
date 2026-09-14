//! Bridges the buildpack-based packages (zlib/expat/libpng/freetype/
//! fontconfig/cairo/xkeyboard-config/weston — weston's mandatory cairo
//! dependency chain, plus the XKB data weston needs at runtime) into
//! `distro`'s own `build-userland` command, so a single `distro
//! build-userland` (and `distro all`) builds everything instead of
//! needing a separate `cargo run -p buildpacks --example weston_chain`
//! step.
//!
//! Deliberately NOT a full cutover: kernel/util-linux/mesa still build
//! through the old `distro/src/stages/{kernel,userland}.rs` path (their
//! buildpack versions remain proof-of-concept only, proven equivalent in
//! `buildpacks/examples/verify.rs`, not switched to). These 8 packages
//! have no old-pipeline equivalent at all, so wiring them in here is
//! pure addition, not a replacement of anything.
//!
//! Reads `distro.toml` a second time as a raw `toml::Value` (not through
//! `Config`, which doesn't model these 8 sections) purely to hand each
//! buildpack its own `[section]` table — everything else (paths, jobs)
//! comes from the already-loaded `Config`, since the buildpacks' own
//! `sources_dir`/`sysroot_dir`/`rootfs_dir` conventions were built to
//! match `Config`'s existing methods exactly (see `weston_chain.rs`'s
//! original verification runner, which this supersedes as the real
//! entry point).

use crate::config::Config;
use anyhow::{Context, Result};
use buildpack_core::{BuildCtx, Buildpack};
use buildpacks::cairo::Cairo;
use buildpacks::expat::Expat;
use buildpacks::fontconfig::Fontconfig;
use buildpacks::freetype::Freetype;
use buildpacks::libpng::Libpng;
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

pub fn build_new_packages(config_path: &Path, cfg: &Config, force: bool) -> Result<()> {
    let text = std::fs::read_to_string(config_path)
        .with_context(|| format!("reading {}", config_path.display()))?;
    let root: toml::Value =
        toml::from_str(&text).with_context(|| format!("parsing {}", config_path.display()))?;

    let packs: Vec<Box<dyn Buildpack>> = vec![
        Box::new(configured::<Zlib>(&root, "zlib")?),
        Box::new(configured::<Expat>(&root, "expat")?),
        Box::new(configured::<Libpng>(&root, "libpng")?),
        Box::new(configured::<Freetype>(&root, "freetype")?),
        Box::new(configured::<Fontconfig>(&root, "fontconfig")?),
        Box::new(configured::<Cairo>(&root, "cairo")?),
        Box::new(configured::<XkeyboardConfig>(&root, "xkeyboard_config")?),
        Box::new(configured::<Weston>(&root, "weston")?),
    ];

    let order = buildpack_core::graph::topo_order(&packs)?;

    let ctx = BuildCtx {
        sources_dir: cfg.sources_dir(),
        build_dir: cfg.build_dir.clone(),
        sysroot_dir: cfg.sysroot_dir(),
        rootfs_dir: cfg.rootfs_dir(),
        arch: cfg.image.arch.clone(),
        networking: cfg.networking,
        jobs: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
    };

    for &i in &order {
        let pack = &packs[i];
        pack.fetch(&ctx, force).with_context(|| format!("fetching buildpack {}", pack.id()))?;
        pack.build(&ctx, force).with_context(|| format!("building buildpack {}", pack.id()))?;
    }

    Ok(())
}
