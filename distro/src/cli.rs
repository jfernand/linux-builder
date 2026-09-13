use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "distro", about = "Builds a from-scratch glibc distro with a COSMIC desktop")]
pub struct Cli {
    /// Path to the config file
    #[arg(long, global = true, default_value = "distro.toml")]
    pub config: PathBuf,

    /// Re-run a stage even if its output already exists
    #[arg(long, global = true)]
    pub force: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Download and extract sources (kernel today; userland sources once
    /// Phase 1 is implemented)
    Fetch,
    /// Ensure the host build toolchain is available (not yet implemented —
    /// see Phase 1 of the roadmap)
    BuildToolchain,
    /// Configure and build the kernel (reuses builder-core's kernel stage
    /// unchanged)
    BuildKernel,
    /// Build the glibc userland: uutils/coreutils, util-linux, shadow-utils
    /// (not yet implemented — see Phase 1 of the roadmap)
    BuildUserland,
    /// Assemble the root filesystem tree (not yet implemented — see Phase 1
    /// of the roadmap)
    AssembleRootfs,
    /// Partition and populate the bootable disk image (reuses
    /// builder-core's image stage unchanged)
    MakeImage,
    /// Boot the produced image in QEMU
    TestQemu {
        /// Open QEMU's own graphical window instead of attaching the serial
        /// console to this terminal
        #[arg(long)]
        window: bool,
    },
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
}
