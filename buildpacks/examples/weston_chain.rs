//! Fetches and builds the 7 new packages weston's mandatory cairo
//! dependency pulled in (zlib, expat, libpng, freetype, fontconfig,
//! cairo, weston itself), in dependency order, against the real
//! `distro.toml` and the real `build-distro/sysroot` — coexisting with
//! everything the old `distro/src/stages` pipeline already built there
//! (pixman, wayland, libdisplay-info, libinput, libdrm, mesa, ...).
//!
//! Run from the repo root: `cargo run -p buildpacks --example weston_chain`.

use anyhow::{Context, Result};
use buildpack_core::{BuildCtx, Buildpack};
use buildpacks::cairo::Cairo;
use buildpacks::expat::Expat;
use buildpacks::fontconfig::Fontconfig;
use buildpacks::freetype::Freetype;
use buildpacks::libpng::Libpng;
use buildpacks::weston::Weston;
use buildpacks::zlib::Zlib;
use std::path::PathBuf;

fn jobs() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
}

fn load_table(path: &str) -> Result<toml::Value> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
    toml::from_str::<toml::Value>(&text).with_context(|| format!("parsing {path}"))
}

fn configured<T: Buildpack + Default>(root: &toml::Value, key: &str) -> Result<T> {
    let mut bp = T::default();
    let empty = toml::Value::Table(Default::default());
    let table = root.get(key).unwrap_or(&empty);
    bp.configure(table)?;
    Ok(bp)
}

fn main() -> Result<()> {
    let root = load_table("distro.toml")?;

    let packs: Vec<Box<dyn Buildpack>> = vec![
        Box::new(configured::<Zlib>(&root, "zlib")?),
        Box::new(configured::<Expat>(&root, "expat")?),
        Box::new(configured::<Libpng>(&root, "libpng")?),
        Box::new(configured::<Freetype>(&root, "freetype")?),
        Box::new(configured::<Fontconfig>(&root, "fontconfig")?),
        Box::new(configured::<Cairo>(&root, "cairo")?),
        Box::new(configured::<Weston>(&root, "weston")?),
    ];

    let order = buildpack_core::graph::topo_order(&packs)?;
    println!(
        "build order: {:?}",
        order.iter().map(|&i| packs[i].id()).collect::<Vec<_>>()
    );

    let svg_path = PathBuf::from("build-distro/dependency-graph.svg");
    buildpack_core::graph::write_svg(&packs, &svg_path)?;
    println!("wrote dependency graph to {}", svg_path.display());

    let ctx = BuildCtx {
        sources_dir: PathBuf::from("build-distro/sources"),
        build_dir: PathBuf::from("build-distro"),
        sysroot_dir: PathBuf::from("build-distro/sysroot"),
        rootfs_dir: PathBuf::from("build-distro/rootfs"),
        arch: "x86_64".into(),
        networking: false,
        jobs: jobs(),
    };

    for &i in &order {
        let pack = &packs[i];
        println!("\n=== {} ===", pack.id());
        pack.fetch(&ctx, false)?;
        pack.build(&ctx, false)?;
        if !pack.is_built(&ctx) {
            anyhow::bail!("{} reports not built after build() succeeded", pack.id());
        }
        println!("=== {} done ===", pack.id());
    }

    println!("\nall 7 packages built successfully.");
    Ok(())
}
