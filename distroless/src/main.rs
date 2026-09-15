mod cli;
mod pipeline;
mod rootfs;
mod stages;
mod tui;

use anyhow::{bail, Result};
use buildpack_core::config::DistroConfig;
use buildpack_core::pipeline::{MakeImage, PipelineStage, TestQemu, WriteUsb};
use buildpack_core::Buildpack;
use clap::Parser;
use cli::{Cli, Command};
use std::io::Write;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = DistroConfig::load(&cli.config)?;

    match cli.command {
        Command::Fetch { clean: _ } => fetch(&cfg, cli.force),
        Command::BuildToolchain => stages::toolchain::build_toolchain(),
        Command::ResolveKernel { channel } => resolve_kernel(&cli.config, channel),
        Command::BuildKernel => build_kernel(&cfg, cli.force),
        Command::MenuConfig { save_to } => {
            let bp = pipeline::kernel_buildpack(&cfg)?;
            bp.menuconfig(&pipeline::kernel_ctx(&cfg), &save_to)
        }
        Command::BuildUserland => pipeline::build_new_packages(&cfg, cli.force),
        Command::AssembleRootfs => rootfs::assemble_rootfs(&cfg, cli.force),
        Command::MakeImage => MakeImage.run(&pipeline::pipeline_ctx(&cfg)?, cli.force),
        Command::TestQemu { window } => TestQemu { window }.run(&pipeline::pipeline_ctx(&cfg)?, cli.force),
        Command::ListDevices => list_devices(),
        Command::ListFeatures => list_features(),
        Command::WriteUsb { device, yes } => write_usb(&cfg, &device, yes),
        Command::All => run_all(&cfg, cli.force),
        Command::Tui => tui::run(cli.config.clone()),
    }
}

/// `distroless`'s own wrapper around `buildpacks::kernel::latest_release`
/// — patches just the `[kernel]` table's `version`/`url` fields via
/// `DistroConfig`, mirroring `distro`'s equivalent
/// (`distro/src/stages/kernel.rs::resolve_kernel`).
fn resolve_kernel(config_path: &std::path::Path, channel: buildpacks::kernel::KernelChannel) -> Result<()> {
    let (version, url) = buildpacks::kernel::latest_release(channel)?;

    let mut cfg = DistroConfig::load(config_path)?;
    let mut kernel = cfg.package_table("kernel");
    let table = kernel.as_table_mut().expect("[kernel] is a table");
    table.insert("version".to_string(), toml::Value::String(version));
    table.insert("url".to_string(), toml::Value::String(url));
    cfg.packages.insert("kernel".to_string(), kernel);
    cfg.save(config_path)?;

    println!("wrote kernel.version/url to {}", config_path.display());
    println!(
        "if you already fetched a different version's sources, remove the old \
         kernel source directory and re-run `fetch` (distroless's fetch has no --clean flag)"
    );
    Ok(())
}

/// `--clean` (remove old sources first) isn't ported — the old pipeline's
/// `clean_sources` deleted per-package build directories by their old
/// `Config` methods; those are gone now that packages own their own
/// paths. A `distro`-style buildpack `clean()` exists (see
/// `Buildpack::clean`) but isn't wired to this flag yet.
fn fetch(cfg: &DistroConfig, force: bool) -> Result<()> {
    let kernel = pipeline::kernel_buildpack(cfg)?;
    kernel.fetch(&pipeline::kernel_ctx(cfg), force)?;
    pipeline::fetch_new_packages(cfg, force)
}

fn build_kernel(cfg: &DistroConfig, force: bool) -> Result<()> {
    let bp = pipeline::kernel_buildpack(cfg)?;
    bp.build(&pipeline::kernel_ctx(cfg), force)
}

fn run_all(cfg: &DistroConfig, force: bool) -> Result<()> {
    fetch(cfg, force)?;
    stages::toolchain::build_toolchain()?;
    build_kernel(cfg, force)?;
    pipeline::build_new_packages(cfg, force)?;
    rootfs::assemble_rootfs(cfg, force)?;
    MakeImage.run(&pipeline::pipeline_ctx(cfg)?, force)?;
    println!("done. run `distroless test-qemu` to boot the image in QEMU.");
    Ok(())
}

fn list_features() -> Result<()> {
    for pack in buildpacks::kernel::FEATURE_PACKS {
        println!("{:<10} {}", pack.key, pack.description);
    }
    Ok(())
}

fn list_devices() -> Result<()> {
    let devices = buildpack_core::pipeline::list_removable_devices()?;
    if devices.is_empty() {
        println!("no removable disks found");
        return Ok(());
    }
    for d in devices {
        println!(
            "{}\t{}\t{}\t{}",
            d.path(),
            d.size,
            d.tran,
            if d.model.is_empty() { "-" } else { &d.model }
        );
    }
    Ok(())
}

fn write_usb(cfg: &DistroConfig, device: &str, yes: bool) -> Result<()> {
    if !yes {
        let devices = buildpack_core::pipeline::list_removable_devices()?;
        let matched = devices.iter().find(|d| d.path() == device);
        match matched {
            Some(d) => println!(
                "About to overwrite {} ({}, {}, {}) with {}",
                d.path(),
                d.size,
                d.tran,
                if d.model.is_empty() { "-" } else { &d.model },
                cfg.output_image().display()
            ),
            None => println!(
                "warning: {device} was not found in the removable-disk list — \
                 double check this is really a USB stick, not your main disk"
            ),
        }
        print!("This will PERMANENTLY ERASE {device}. Type the device path again to confirm: ");
        std::io::stdout().flush().ok();
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if input.trim() != device {
            bail!("confirmation did not match {device}, aborting");
        }
    }

    WriteUsb { device: device.to_string(), confirmed: true }.run(&pipeline::pipeline_ctx(cfg)?, false)
}
