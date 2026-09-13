use builder_core::stages::KernelChannel;
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
    /// Look up the current stable or LTS kernel release on kernel.org and
    /// write its version/url into the config file's [kernel] section
    ResolveKernel {
        #[arg(long, value_enum)]
        channel: KernelChannel,
    },
    /// Configure and build the kernel
    BuildKernel,
    /// Interactively customize the kernel config with `make menuconfig` and
    /// save the result for reuse via `kernel.config_file`
    MenuConfig {
        /// Where to save the resulting kernel config
        #[arg(long, default_value = "kernel.config")]
        save_to: PathBuf,
    },
    /// Build uutils (musl, static) and busybox (musl, static)
    BuildUserland,
    /// Assemble the root filesystem tree
    AssembleRootfs,
    /// Partition and populate the bootable disk image
    MakeImage,
    /// Boot the produced image in QEMU
    TestQemu {
        /// Open QEMU's own graphical window instead of attaching the serial
        /// console to this terminal (used by the TUI, which otherwise can't
        /// pipe keyboard input to an interactive boot)
        #[arg(long)]
        window: bool,
    },
    /// List removable disks that look like USB sticks
    ListDevices,
    /// List named kernel feature packs that can be enabled via
    /// `kernel.features` in the config file (or the TUI settings screen)
    ListFeatures,
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
