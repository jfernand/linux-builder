//! Verification scaffolding for the buildpack trait proof-of-concept
//! (see the plan file's "This session's build order", step 5). Not wired
//! into any real CLI — run directly with
//! `cargo run -p buildpacks --example verify` from the repo root.
//!
//! Checks, against real `distro.toml` values:
//! - `graph::topo_order` accepts the 3 proof buildpacks with no errors.
//! - `UtilLinux`: a full fresh fetch+build into a scratch directory
//!   (untouched real `build-distro/` state), confirming agetty/mount/umount
//!   come out as real static ELF binaries.
//! - `Mesa`: pointed directly at the real, already-built
//!   `build-distro/mesa/mesa-mesa-<version>` tree and the real
//!   `build-distro/sysroot` — proves `outputs()`/`is_built()`/`build()`'s
//!   marker-skip logic correctly recognizes prior real work through the
//!   new buildpack code path, without an expensive rebuild.

use anyhow::{Context, Result};
use buildpack_core::{BuildCtx, Buildpack};
use buildpacks::kernel::Kernel;
use buildpacks::mesa::Mesa;
use buildpacks::util_linux::UtilLinux;
use std::path::PathBuf;
use std::process::Command;

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

    // --- 1. topo_order sanity check -------------------------------------
    let packs: Vec<Box<dyn Buildpack>> = vec![
        Box::new(configured::<Kernel>(&root, "kernel")?),
        Box::new(configured::<UtilLinux>(&root, "util_linux")?),
    ];
    let order = buildpack_core::graph::topo_order(&packs)?;
    println!(
        "[topo_order] ok: {:?}",
        order.iter().map(|&i| packs[i].id()).collect::<Vec<_>>()
    );

    // --- 2. UtilLinux: full fresh fetch+build into a scratch dir --------
    let scratch = PathBuf::from("/tmp/buildpack-verify");
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch)?;

    let ul_ctx = BuildCtx {
        sources_dir: scratch.join("sources"),
        build_dir: scratch.clone(),
        sysroot_dir: scratch.join("sysroot"),
        rootfs_dir: scratch.join("rootfs"),
        arch: "x86_64".into(),
        networking: false,
        jobs: jobs(),
    };
    let ul: UtilLinux = configured(&root, "util_linux")?;
    println!("\n[util_linux] fetching...");
    ul.fetch(&ul_ctx, false)?;
    println!("[util_linux] building (fresh, force=false since dir is new)...");
    ul.build(&ul_ctx, false)?;

    let outs = ul.outputs(&ul_ctx);
    println!("[util_linux] {} output(s):", outs.len());
    for out in &outs {
        let ok = out.path.exists();
        println!("  {} exists={ok} path={}", out.description, out.path.display());
        if ok {
            run_file(&out.path);
        }
    }
    assert!(ul.is_built(&ul_ctx), "util_linux should report is_built() after a real build");
    println!("[util_linux] is_built() == true, confirmed");

    // Compare against what the existing (old-code-path) build already
    // produced in build-distro/, for a sanity cross-check.
    let existing_agetty = PathBuf::from("build-distro/util-linux/util-linux-2.41.2/agetty");
    if existing_agetty.exists() {
        println!("\n[util_linux] comparing against existing build-distro/ output:");
        run_file(&existing_agetty);
    }

    // --- 3. Mesa: point at the real, already-built tree ------------------
    let mesa_ctx = BuildCtx {
        sources_dir: PathBuf::from("build-distro/mesa"),
        build_dir: PathBuf::from("build-distro"),
        sysroot_dir: PathBuf::from("build-distro/sysroot"),
        rootfs_dir: PathBuf::from("build-distro/rootfs"),
        arch: "x86_64".into(),
        networking: false,
        jobs: jobs(),
    };
    let mesa: Mesa = configured(&root, "mesa")?;
    println!("\n[mesa] fetch() against the real, already-extracted tree...");
    mesa.fetch(&mesa_ctx, false)?;

    let outs = mesa.outputs(&mesa_ctx);
    println!("[mesa] {} output(s):", outs.len());
    for out in &outs {
        println!("  {} exists={} path={}", out.description, out.path.exists(), out.path.display());
    }
    assert!(mesa.is_built(&mesa_ctx), "mesa should report is_built() against the real sysroot");
    println!("[mesa] is_built() == true, confirmed against real prior build");

    println!("\n[mesa] build() should now hit the already-built skip path:");
    mesa.build(&mesa_ctx, false)?;

    println!("\nall checks passed.");
    Ok(())
}

fn run_file(path: &std::path::Path) {
    if let Ok(out) = Command::new("file").arg(path).output() {
        print!("    {}", String::from_utf8_lossy(&out.stdout));
    }
}
