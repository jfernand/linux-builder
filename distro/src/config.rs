use anyhow::{Context, Result};
use builder_core::config::{BusyboxConfig, ImageConfig, KernelConfig, UutilsConfig};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// `distro`'s own config shape. Deliberately does *not* reuse
/// `builder_core::config::Config` wholesale — that struct requires a
/// `[busybox]`/`[uutils]` section neither of which `distro` has any use
/// for (see `to_builder_core` below). `KernelConfig`/`ImageConfig` are
/// reused directly since they're genuinely identical between the two
/// distros.
#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    pub kernel: KernelConfig,
    pub image: ImageConfig,
    #[serde(default = "default_build_dir")]
    pub build_dir: PathBuf,
    #[serde(default)]
    pub networking: bool,
}

/// Deliberately distinct from `distroless`'s default (`build`) so running
/// both from the same checkout doesn't have them overwrite each other's
/// rootfs/image outputs.
fn default_build_dir() -> PathBuf {
    PathBuf::from("build-distro")
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config file {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing config file {}", path.display()))
    }

    #[allow(dead_code)] // reserved for a future settings-editing command/TUI
    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).context("serializing config")?;
        std::fs::write(path, text)
            .with_context(|| format!("writing config file {}", path.display()))
    }

    pub fn sources_dir(&self) -> PathBuf {
        self.build_dir.join("sources")
    }

    pub fn kernel_build_dir(&self) -> PathBuf {
        self.build_dir
            .join("kernel")
            .join(format!("linux-{}", self.kernel.version))
    }

    #[allow(dead_code)] // used once assemble_rootfs is implemented (Phase 1)
    pub fn rootfs_dir(&self) -> PathBuf {
        self.build_dir.join("rootfs")
    }

    pub fn output_image(&self) -> PathBuf {
        self.build_dir.join("output.img")
    }

    /// Adapts to `builder_core::config::Config`, for calling the reused
    /// generic stage functions (`build_kernel`, `make_image`, `test_qemu`,
    /// `write_usb`) — none of which read the busybox/uutils sections, so
    /// these are harmless placeholders rather than something `distro`'s
    /// own config file needs to carry.
    pub fn to_builder_core(&self) -> builder_core::config::Config {
        builder_core::config::Config {
            kernel: self.kernel.clone(),
            busybox: BusyboxConfig { version: String::new(), url: String::new() },
            uutils: UutilsConfig { git_url: String::new(), git_rev: String::new() },
            image: self.image.clone(),
            build_dir: self.build_dir.clone(),
            networking: self.networking,
        }
    }
}
