mod cli;
mod config;
mod stages;

use anyhow::{bail, Result};
use clap::Parser;
use cli::{Cli, Command};
use config::Config;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = Config::load(&cli.config)?;

    match cli.command {
        Command::Fetch => stages::fetch::fetch(&cfg, cli.force),
        Command::BuildToolchain => stages::toolchain::build_toolchain(),
        Command::BuildKernel => not_yet_implemented("build-kernel"),
        Command::BuildUserland => stages::userland::build_userland(&cfg, cli.force),
        Command::AssembleRootfs => stages::rootfs::assemble_rootfs(&cfg, cli.force),
        Command::MakeImage => not_yet_implemented("make-image"),
        Command::TestQemu { .. } => not_yet_implemented("test-qemu"),
        Command::ListDevices => not_yet_implemented("list-devices"),
        Command::WriteUsb { .. } => not_yet_implemented("write-usb"),
        Command::All => not_yet_implemented("all"),
    }
}

fn not_yet_implemented(command: &str) -> Result<()> {
    bail!("{command}: not yet implemented (see the distro roadmap)")
}
