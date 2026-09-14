mod cli;
mod stages;

use anyhow::{bail, Result};
use buildpack_core::config::DistroConfig;
use buildpack_core::Buildpack;
use clap::Parser;
use cli::{Cli, Command};
use std::io::Write;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = DistroConfig::load(&cli.config)?;

    match cli.command {
        Command::Fetch => fetch(&cfg, cli.force),
        Command::BuildToolchain => stages::toolchain::build_toolchain(),
        Command::ResolveKernel { channel } => stages::kernel::resolve_kernel(&cli.config, channel),
        Command::BuildKernel => build_kernel(&cfg, cli.force),
        Command::MenuConfig { save_to } => {
            let bp = stages::buildpacks::kernel_buildpack(&cfg)?;
            bp.menuconfig(&stages::buildpacks::kernel_ctx(&cfg), &save_to)
        }
        Command::BuildUserland => stages::buildpacks::build_new_packages(&cfg, cli.force),
        Command::AssembleRootfs => stages::rootfs::assemble_rootfs(&cfg, cli.force),
        Command::MakeImage => builder_core::stages::image::make_image(&to_builder_core(&cfg), cli.force),
        Command::TestQemu { window } => {
            builder_core::stages::qemu::test_qemu(&to_builder_core(&cfg), window)
        }
        Command::ListDevices => list_devices(),
        Command::ListFeatures => list_features(),
        Command::WriteUsb { device, yes } => write_usb(&cfg, &device, yes),
        Command::All => run_all(&cfg, cli.force),
    }
}

/// Adapts to `builder_core::config::Config`, for calling the reused
/// generic stage functions (`make_image`/`test_qemu`/`write_usb`) — none
/// of which read the kernel/busybox/uutils sections, so these are
/// harmless placeholders. Goes away once Phase 3 replaces these with
/// `PipelineStage` impls that take a plain `BuildCtx` instead.
fn to_builder_core(cfg: &DistroConfig) -> builder_core::config::Config {
    builder_core::config::Config {
        kernel: builder_core::config::KernelConfig {
            version: String::new(),
            url: String::new(),
            config_file: None,
            features: Vec::new(),
            logo_file: None,
        },
        busybox: builder_core::config::BusyboxConfig { version: String::new(), url: String::new() },
        uutils: builder_core::config::UutilsConfig { git_url: String::new(), git_rev: String::new() },
        image: builder_core::config::ImageConfig {
            arch: cfg.image.arch.clone(),
            size_mb: cfg.image.size_mb,
            hostname: cfg.image.hostname.clone(),
        },
        build_dir: cfg.build_dir.clone(),
        networking: cfg.networking,
    }
}

fn fetch(cfg: &DistroConfig, force: bool) -> Result<()> {
    let kernel = stages::buildpacks::kernel_buildpack(cfg)?;
    kernel.fetch(&stages::buildpacks::kernel_ctx(cfg), force)?;
    stages::buildpacks::fetch_new_packages(cfg, force)
}

fn build_kernel(cfg: &DistroConfig, force: bool) -> Result<()> {
    let bp = stages::buildpacks::kernel_buildpack(cfg)?;
    bp.build(&stages::buildpacks::kernel_ctx(cfg), force)
}

fn run_all(cfg: &DistroConfig, force: bool) -> Result<()> {
    fetch(cfg, force)?;
    stages::toolchain::build_toolchain()?;
    build_kernel(cfg, force)?;
    stages::buildpacks::build_new_packages(cfg, force)?;
    stages::rootfs::assemble_rootfs(cfg, force)?;
    builder_core::stages::image::make_image(&to_builder_core(cfg), force)?;
    println!("done. run `distro test-qemu` to boot the image in QEMU.");
    Ok(())
}

fn list_features() -> Result<()> {
    for pack in buildpacks::kernel::FEATURE_PACKS {
        println!("{:<10} {}", pack.key, pack.description);
    }
    Ok(())
}

fn list_devices() -> Result<()> {
    let devices = builder_core::stages::usb::list_removable_devices()?;
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
    let builder_cfg = to_builder_core(cfg);

    if !yes {
        let devices = builder_core::stages::usb::list_removable_devices()?;
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

    builder_core::stages::usb::write_usb(&builder_cfg, device, true, |line| println!("{line}"))
}
