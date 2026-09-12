use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    pub kernel: KernelConfig,
    pub busybox: BusyboxConfig,
    pub uutils: UutilsConfig,
    pub image: ImageConfig,
    #[serde(default = "default_build_dir")]
    pub build_dir: PathBuf,
    /// Whether to build in DHCP networking support (busybox udhcpc/ifconfig/
    /// route/ping, a udhcpc boot script, and a QEMU NIC for testing).
    /// Toggleable from the TUI settings screen; off by default so existing
    /// images are unaffected until you opt in.
    #[serde(default)]
    pub networking: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct KernelConfig {
    pub version: String,
    pub url: String,
    /// Path to a saved `.config` (e.g. produced by `menuconfig`) to build
    /// with instead of `defconfig`. Optional; unset builds the stock config.
    #[serde(default)]
    pub config_file: Option<PathBuf>,
    /// Named feature packs to enable (see `stages::kernel::FEATURE_PACKS`),
    /// e.g. `["graphics"]`. Applied on top of `config_file`/`defconfig`.
    #[serde(default)]
    pub features: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct BusyboxConfig {
    pub version: String,
    pub url: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UutilsConfig {
    pub git_url: String,
    pub git_rev: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ImageConfig {
    #[allow(dead_code)] // reserved for future multi-arch support
    pub arch: String,
    pub size_mb: u64,
    pub hostname: String,
}

fn default_build_dir() -> PathBuf {
    PathBuf::from("build")
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config file {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing config file {}", path.display()))
    }

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

    pub fn busybox_build_dir(&self) -> PathBuf {
        self.build_dir
            .join("busybox")
            .join(format!("busybox-{}", self.busybox.version))
    }

    pub fn uutils_build_dir(&self) -> PathBuf {
        self.build_dir.join("uutils")
    }

    pub fn rootfs_dir(&self) -> PathBuf {
        self.build_dir.join("rootfs")
    }

    pub fn output_image(&self) -> PathBuf {
        self.build_dir.join("output.img")
    }
}
