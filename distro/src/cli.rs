use buildpacks::kernel::KernelChannel;
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
    /// Download and extract every source tarball
    Fetch,
    /// Ensure the host build toolchain is available
    BuildToolchain,
    /// Look up the current stable or LTS kernel release on kernel.org and
    /// write its version/url into the config file's [kernel] section
    ResolveKernel {
        #[arg(long, value_enum)]
        channel: KernelChannel,
    },
    /// Configure and build the kernel (reuses builder-core's kernel stage
    /// unchanged)
    BuildKernel,
    /// Interactively customize the kernel config with `make menuconfig` and
    /// save the result for reuse via `kernel.config_file`
    MenuConfig {
        /// Where to save the resulting kernel config
        #[arg(long, default_value = "kernel.config")]
        save_to: PathBuf,
    },
    /// Build the glibc userland: uutils/coreutils, bash, util-linux,
    /// shadow-utils, and the seat/session/Wayland-core packages
    BuildUserland,
    /// Assemble the root filesystem tree
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
    /// List named kernel feature packs that can be enabled via
    /// `kernel.features` in the config file
    ListFeatures,
    /// List every buildpack this distro builds, with its build status
    ListPackages,
    /// Fetch (download/extract/patch) one package by id, ignoring every
    /// other package
    FetchPkg {
        id: String,
    },
    /// Build one package by id, ignoring every other package (does not
    /// fetch it first — run fetch-pkg or fetch first)
    BuildPkg {
        id: String,
    },
    /// Remove one package's build outputs so it rebuilds next run
    CleanPkg {
        id: String,
    },
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
