mod cli;
mod config;
mod stages;

use anyhow::{bail, Result};
use buildpack_core::Buildpack;
use clap::Parser;
use cli::{Cli, Command};
use config::Config;
use std::io::Write;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = Config::load(&cli.config)?;

    match cli.command {
        Command::Fetch => fetch(&cli.config, &cfg, cli.force),
        Command::BuildToolchain => stages::toolchain::build_toolchain(),
        Command::ResolveKernel { channel } => stages::kernel::resolve_kernel(&cli.config, channel),
        Command::BuildKernel => build_kernel(&cli.config, &cfg, cli.force),
        Command::MenuConfig { save_to } => {
            let bp = stages::buildpacks::kernel_buildpack(&cli.config)?;
            bp.menuconfig(&stages::buildpacks::kernel_ctx(&cfg), &save_to)
        }
        Command::BuildUserland => stages::buildpacks::build_new_packages(&cli.config, &cfg, cli.force),
        Command::AssembleRootfs => stages::rootfs::assemble_rootfs(&cli.config, &cfg, cli.force),
        Command::MakeImage => builder_core::stages::image::make_image(&cfg.to_builder_core(), cli.force),
        Command::TestQemu { window } => builder_core::stages::qemu::test_qemu(&cfg.to_builder_core(), window),
        Command::ListDevices => list_devices(),
        Command::ListFeatures => list_features(),
        Command::WriteUsb { device, yes } => write_usb(&cfg, &device, yes),
        Command::All => run_all(&cli.config, &cfg, cli.force),
    }
}

fn fetch(config_path: &std::path::Path, cfg: &Config, force: bool) -> Result<()> {
    let kernel = stages::buildpacks::kernel_buildpack(config_path)?;
    kernel.fetch(&stages::buildpacks::kernel_ctx(cfg), force)?;
    stages::buildpacks::fetch_new_packages(config_path, cfg, force)
}

fn build_kernel(config_path: &std::path::Path, cfg: &Config, force: bool) -> Result<()> {
    let bp = stages::buildpacks::kernel_buildpack(config_path)?;
    bp.build(&stages::buildpacks::kernel_ctx(cfg), force)
}

fn run_all(config_path: &std::path::Path, cfg: &Config, force: bool) -> Result<()> {
    fetch(config_path, cfg, force)?;
    stages::toolchain::build_toolchain()?;
    build_kernel(config_path, cfg, force)?;
    stages::buildpacks::build_new_packages(config_path, cfg, force)?;
    stages::rootfs::assemble_rootfs(config_path, cfg, force)?;
    builder_core::stages::image::make_image(&cfg.to_builder_core(), force)?;
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

fn write_usb(cfg: &Config, device: &str, yes: bool) -> Result<()> {
    let builder_cfg = cfg.to_builder_core();

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
