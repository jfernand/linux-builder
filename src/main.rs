mod cli;
mod config;
mod stages;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};
use config::Config;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = Config::load(&cli.config)?;

    match cli.command {
        Command::Fetch => stages::fetch::fetch(&cfg, cli.force),
        Command::BuildToolchain => stages::toolchain::build_toolchain(),
        Command::BuildKernel => stages::kernel::build_kernel(&cfg, cli.force),
        Command::BuildUserland => stages::userland::build_userland(&cfg, cli.force),
        Command::AssembleRootfs => stages::rootfs::assemble_rootfs(&cfg, cli.force),
        Command::MakeImage => stages::image::make_image(&cfg, cli.force),
        Command::TestQemu => stages::qemu::test_qemu(&cfg),
        Command::All => run_all(&cfg, cli.force),
    }
}

fn run_all(cfg: &Config, force: bool) -> Result<()> {
    stages::fetch::fetch(cfg, force)?;
    stages::toolchain::build_toolchain()?;
    stages::kernel::build_kernel(cfg, force)?;
    stages::userland::build_userland(cfg, force)?;
    stages::rootfs::assemble_rootfs(cfg, force)?;
    stages::image::make_image(cfg, force)?;
    println!("done. run `linux-builder test-qemu` to boot the image in QEMU.");
    Ok(())
}
