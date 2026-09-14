use anyhow::{Context, Result};
use builder_core::config::{BusyboxConfig, ImageConfig, KernelConfig, UutilsConfig};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// `distro`'s own config shape. Deliberately does *not* reuse
/// `builder_core::config::Config` wholesale — that struct requires a
/// `[busybox]` section `distro` has no use for (see `to_builder_core`
/// below). `KernelConfig`/`ImageConfig`/`UutilsConfig` are reused directly
/// since they're genuinely identical between the two distros; `bash` is
/// distro-specific (distroless uses BusyBox's `ash` instead).
#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    pub kernel: KernelConfig,
    pub uutils: UutilsConfig,
    pub bash: BashConfig,
    pub util_linux: UtilLinuxConfig,
    pub shadow: ShadowConfig,
    pub seatd: SeatdConfig,
    pub dbus: DbusConfig,
    pub eudev: EudevConfig,
    pub image: ImageConfig,
    #[serde(default = "default_build_dir")]
    pub build_dir: PathBuf,
    #[serde(default)]
    pub networking: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BashConfig {
    pub version: String,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UtilLinuxConfig {
    pub version: String,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ShadowConfig {
    pub version: String,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SeatdConfig {
    pub version: String,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DbusConfig {
    pub version: String,
    pub url: String,
}

/// eudev: a systemd-independent fork of udev, providing libudev for
/// libinput (Phase 2's real device-manager daemon, alongside seatd/dbus).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EudevConfig {
    pub version: String,
    pub url: String,
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

    pub fn uutils_build_dir(&self) -> PathBuf {
        self.build_dir.join("uutils")
    }

    pub fn bash_build_dir(&self) -> PathBuf {
        self.build_dir.join("bash").join(format!("bash-{}", self.bash.version))
    }

    pub fn util_linux_build_dir(&self) -> PathBuf {
        self.build_dir
            .join("util-linux")
            .join(format!("util-linux-{}", self.util_linux.version))
    }

    pub fn shadow_build_dir(&self) -> PathBuf {
        self.build_dir.join("shadow").join(format!("shadow-{}", self.shadow.version))
    }

    pub fn seatd_build_dir(&self) -> PathBuf {
        self.build_dir.join("seatd").join(format!("seatd-{}", self.seatd.version))
    }

    pub fn dbus_build_dir(&self) -> PathBuf {
        self.build_dir.join("dbus").join(format!("dbus-{}", self.dbus.version))
    }

    pub fn eudev_build_dir(&self) -> PathBuf {
        self.build_dir.join("eudev").join(format!("eudev-{}", self.eudev.version))
    }

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
            uutils: self.uutils.clone(),
            image: self.image.clone(),
            build_dir: self.build_dir.clone(),
            networking: self.networking,
        }
    }
}
