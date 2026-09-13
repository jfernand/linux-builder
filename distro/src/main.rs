mod cli;
mod config;
mod stages;

use anyhow::{bail, Result};
use clap::Parser;
use cli::{Cli, Command};
use config::Config;
use std::io::Write;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = Config::load(&cli.config)?;

    match cli.command {
        Command::Fetch => stages::fetch::fetch(&cfg, cli.force),
        Command::BuildToolchain => stages::toolchain::build_toolchain(),
        Command::BuildKernel => builder_core::stages::kernel::build_kernel(&cfg.to_builder_core(), cli.force),
        Command::BuildUserland => stages::userland::build_userland(&cfg, cli.force),
        Command::AssembleRootfs => stages::rootfs::assemble_rootfs(&cfg, cli.force),
        Command::MakeImage => builder_core::stages::image::make_image(&cfg.to_builder_core(), cli.force),
        Command::TestQemu { window } => builder_core::stages::qemu::test_qemu(&cfg.to_builder_core(), window),
        Command::ListDevices => list_devices(),
        Command::WriteUsb { device, yes } => write_usb(&cfg, &device, yes),
        Command::All => run_all(&cfg, cli.force),
    }
}

fn run_all(cfg: &Config, force: bool) -> Result<()> {
    stages::fetch::fetch(cfg, force)?;
    stages::toolchain::build_toolchain()?;
    builder_core::stages::kernel::build_kernel(&cfg.to_builder_core(), force)?;
    stages::userland::build_userland(cfg, force)?;
    stages::rootfs::assemble_rootfs(cfg, force)?;
    builder_core::stages::image::make_image(&cfg.to_builder_core(), force)?;
    println!("done. run `distro test-qemu` to boot the image in QEMU.");
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
