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
    pub wayland: WaylandConfig,
    pub wayland_protocols: WaylandProtocolsConfig,
    pub libxkbcommon: LibxkbcommonConfig,
    pub pixman: PixmanConfig,
    pub libdisplay_info: LibdisplayInfoConfig,
    pub libevdev: LibevdevConfig,
    pub libinput: LibinputConfig,
    pub libdrm: LibdrmConfig,
    pub mesa: MesaConfig,
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

// The rest of Phase 2: link-time libraries for Phase 3's compositor, none
// of them running as services (no distro-init changes needed for these).

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WaylandConfig {
    pub version: String,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WaylandProtocolsConfig {
    pub version: String,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LibxkbcommonConfig {
    pub version: String,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PixmanConfig {
    pub version: String,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LibdisplayInfoConfig {
    pub version: String,
    pub url: String,
}

/// libinput's mandatory (not optional) dependency for reading raw input
/// devices — built from source rather than taken from the host, same as
/// every other runtime dependency from Phase 2 on.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LibevdevConfig {
    pub version: String,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LibinputConfig {
    pub version: String,
    pub url: String,
}

// Phase 3: the graphics stack, scoped to QEMU's virtio-gpu first.

/// The kernel-userspace ioctl wrapper library every GPU-facing library
/// (Mesa included) builds on.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LibdrmConfig {
    pub version: String,
    pub url: String,
}

/// Built scoped to the `virgl`/`softpipe` gallium drivers only — real GPU
/// drivers (Intel/AMD/nouveau) are explicitly out of scope until this
/// QEMU/virtio-gpu milestone works (see the roadmap's Phase 3).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MesaConfig {
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

    pub fn wayland_build_dir(&self) -> PathBuf {
        self.build_dir.join("wayland").join(format!("wayland-{}", self.wayland.version))
    }

    pub fn wayland_protocols_build_dir(&self) -> PathBuf {
        self.build_dir
            .join("wayland-protocols")
            .join(format!("wayland-protocols-{}", self.wayland_protocols.version))
    }

    /// GitHub's tag archive nests an extra `libxkbcommon-` prefix onto the
    /// tag name (`libxkbcommon-xkbcommon-1.12.4/`), unlike every other
    /// dependency's tarball, which extracts to just `<name>-<version>/`.
    pub fn libxkbcommon_build_dir(&self) -> PathBuf {
        self.build_dir.join("libxkbcommon").join(format!(
            "libxkbcommon-xkbcommon-{}",
            self.libxkbcommon.version
        ))
    }

    /// Same GitLab-archive double-naming as libxkbcommon's GitHub one:
    /// extracts to `pixman-pixman-<version>/`.
    pub fn pixman_build_dir(&self) -> PathBuf {
        self.build_dir.join("pixman").join(format!("pixman-pixman-{}", self.pixman.version))
    }

    pub fn libdisplay_info_build_dir(&self) -> PathBuf {
        self.build_dir
            .join("libdisplay-info")
            .join(format!("libdisplay-info-{}", self.libdisplay_info.version))
    }

    /// Same GitLab-archive double-naming again: `libevdev-libevdev-<version>/`.
    pub fn libevdev_build_dir(&self) -> PathBuf {
        self.build_dir
            .join("libevdev")
            .join(format!("libevdev-libevdev-{}", self.libevdev.version))
    }

    pub fn libinput_build_dir(&self) -> PathBuf {
        self.build_dir.join("libinput").join(format!("libinput-{}", self.libinput.version))
    }

    /// GitLab appends the tag's target commit SHA to libdrm's archive
    /// directory name specifically (unlike pixman's/libevdev's own clean
    /// `<name>-<name>-<version>/` GitLab-archive naming) — a per-project,
    /// per-tag quirk, not something derivable from the version alone.
    /// Literal, not templated with `self.libdrm.version`: bumping the
    /// version here means updating this whole string, since the hash is
    /// only valid for the exact tag it names.
    pub fn libdrm_build_dir(&self) -> PathBuf {
        self.build_dir
            .join("libdrm")
            .join("libdrm-libdrm-2.4.134-e984d448b8b17aab853369e6c203e53719f46de1")
    }

    pub fn mesa_build_dir(&self) -> PathBuf {
        self.build_dir.join("mesa").join(format!("mesa-mesa-{}", self.mesa.version))
    }

    /// Staging install prefix all of Phase 2's meson/autotools packages
    /// install into as part of building (not just at rootfs-assembly
    /// time) — so a later package's build (e.g. wayland-protocols needing
    /// wayland-scanner, libinput needing eudev's libudev) can find an
    /// earlier one via `PKG_CONFIG_PATH`/`PATH`, the same way a real
    /// distro's build pipeline chains packages through a sysroot instead
    /// of the host's own system paths. `assemble_rootfs` copies this
    /// whole tree into the final rootfs verbatim.
    pub fn sysroot_dir(&self) -> PathBuf {
        self.build_dir.join("sysroot")
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
