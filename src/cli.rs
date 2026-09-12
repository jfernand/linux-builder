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
    Fetch {
        /// Remove previously downloaded archives and extracted sources before fetching
        #[arg(long)]
        clean: bool,
    },
    /// Ensure the musl toolchain and cargo musl target are available
    BuildToolchain,
    /// Configure and build the kernel
    BuildKernel,
    /// Build uutils (musl, static) and busybox (musl, static)
    BuildUserland,
    /// Assemble the root filesystem tree
    AssembleRootfs,
    /// Partition and populate the bootable disk image
    MakeImage,
    /// Boot the produced image in QEMU
    TestQemu,
    /// List removable disks that look like USB sticks
    ListDevices,
    /// Write the built image to a removable device (DESTRUCTIVE)
    WriteUsb {
        /// Target device, e.g. /dev/sdb (must be a whole disk, not a partition)
        #[arg(long)]
        device: String,
        /// Skip the interactive confirmation prompt (only for scripted use
        /// after you've already verified the device yourself)
        #[arg(long)]
        yes: bool,
    },
    /// Run the full pipeline end to end
    All,
    /// Interactive dashboard for running every stage and writing to a USB stick
    Tui,
}
