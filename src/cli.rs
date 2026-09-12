use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "linux-builder", about = "Orchestrates building a minimal bootable Linux distro")]
pub struct Cli {
    /// Path to the config file
    #[arg(long, global = true, default_value = "linux-builder.toml")]
    pub config: PathBuf,

    /// Re-run a stage even if its output already exists
    #[arg(long, global = true)]
    pub force: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Download and extract kernel, busybox, and uutils sources
    Fetch,
    /// Ensure the musl toolchain and cargo musl target are available
    BuildToolchain,
}
