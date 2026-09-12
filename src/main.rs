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
    }
}
