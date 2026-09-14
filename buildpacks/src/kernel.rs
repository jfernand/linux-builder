//! The kernel buildpack — ported from `builder-core/src/stages/kernel.rs`,
//! unchanged in logic. `FEATURE_PACKS`/`FeaturePack` stay internal to
//! `Kernel`, exposed via one inherent method (`Kernel::feature_packs()`)
//! rather than becoming their own `Buildpack` impls — see the plan file's
//! "FeaturePack stays internal to Kernel" design decision.

use anyhow::{bail, Context, Result};
use buildpack_core::{
    run::run_in, BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source,
};
use serde::Deserialize;
use std::any::Any;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct KernelConfig {
    pub version: String,
    pub url: String,
    #[serde(default)]
    pub config_file: Option<PathBuf>,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub logo_file: Option<PathBuf>,
}

#[derive(Default)]
pub struct Kernel {
    cfg: KernelConfig,
}

impl Kernel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Matches `default_fetch`'s extraction target (`sources_dir` joined
    /// with the same `extracted_dir_name` used in `sources()` below) —
    /// they must agree for `build()` to find what `fetch()` produced.
    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("linux-{}", self.cfg.version))
    }

    pub fn feature_packs(&self) -> &'static [FeaturePack] {
        FEATURE_PACKS
    }

    /// Interactively runs `make menuconfig`, then saves the resulting
    /// `.config` to `save_to` for reuse as `kernel.config_file`.
    pub fn menuconfig(&self, ctx: &BuildCtx, save_to: &Path) -> Result<()> {
        let dir = self.build_dir(ctx);
        if !dir.exists() {
            bail!("kernel sources not found at {} — run fetch first", dir.display());
        }
        if !dir.join(".config").exists() {
            self.configure_kernel(ctx, &dir)?;
        }

        run_in(&dir, Command::new("make").arg("menuconfig"))?;

        if let Some(parent) = save_to.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(dir.join(".config"), save_to)
            .with_context(|| format!("saving kernel config to {}", save_to.display()))?;

        println!("saved kernel config to {}", save_to.display());
        Ok(())
    }

    fn configure_kernel(&self, _ctx: &BuildCtx, dir: &Path) -> Result<()> {
        let config_path = dir.join(".config");
        let shrink_to_base = match &self.cfg.config_file {
            Some(path) => {
                println!("applying custom kernel config from {}", path.display());
                std::fs::copy(path, &config_path)
                    .with_context(|| format!("copying {} into {}", path.display(), dir.display()))?;
                false
            }
            None => {
                println!("configuring kernel in {}", dir.display());
                run_in(dir, Command::new("make").arg("defconfig"))?;
                true
            }
        };

        let mut text = std::fs::read_to_string(&config_path)
            .with_context(|| format!("reading {}", config_path.display()))?;

        if shrink_to_base {
            for pack in FEATURE_PACKS {
                disable_options(&mut text, pack.options);
            }
        }
        self.enable_features(&mut text)?;

        std::fs::write(&config_path, text)?;
        run_in(dir, Command::new("make").arg("olddefconfig"))?;
        Ok(())
    }

    fn enable_features(&self, text: &mut String) -> Result<()> {
        for key in &self.cfg.features {
            let pack = feature_pack(key)
                .with_context(|| format!("unknown kernel feature \"{key}\""))?;
            println!("enabling kernel feature: {} ({})", pack.label, pack.key);
            for option in pack.options {
                let not_set = format!("# {option} is not set");
                let enabled = format!("{option}=y");
                if text.contains(&not_set) {
                    *text = text.replace(&not_set, &enabled);
                } else if !text.contains(&enabled) {
                    text.push_str(&format!("{enabled}\n"));
                }
            }
        }
        Ok(())
    }

    fn apply_custom_logo(&self, dir: &Path) -> Result<()> {
        let Some(logo_file) = &self.cfg.logo_file else { return Ok(()) };
        if !self.cfg.features.iter().any(|f| f == "boot-logo") {
            return Ok(());
        }
        println!("using custom boot logo from {}", logo_file.display());
        std::fs::copy(logo_file, dir.join(LOGO_PATH))
            .with_context(|| format!("copying {} into {}", logo_file.display(), dir.display()))?;
        Ok(())
    }
}

impl Buildpack for Kernel {
    fn id(&self) -> &'static str {
        "kernel"
    }

    fn configure(&mut self, table: &toml::Value) -> Result<()> {
        self.cfg = table.clone().try_into().context("parsing [kernel] config")?;
        Ok(())
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    fn describe(&self) -> Description {
        Description {
            id: "kernel",
            name: "Linux kernel",
            summary: "Our own kernel build, configured via a curated FEATURE_PACKS set",
            long_description: "Starts from `make defconfig`, strips every optional \
                FEATURE_PACKS option off, then re-enables whichever packs are named \
                in config — same base every distro shares.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("linux-{}.tar.xz", self.cfg.version),
            extracted_dir_name: format!("linux-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> Result<()> {
        let dir = self.build_dir(ctx);
        let bzimage = dir.join("arch/x86/boot/bzImage");

        if buildpack_core::run::already_built(&bzimage, force) {
            println!("skip build-kernel: {} already exists", bzimage.display());
            return Ok(());
        }

        self.configure_kernel(ctx, &dir)?;
        self.apply_custom_logo(&dir)?;

        let jobs = ctx.jobs;
        println!("building kernel ({jobs} jobs)");
        run_in(&dir, Command::new("make").arg(format!("-j{jobs}")))?;

        Ok(())
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "bzImage",
            path: self.build_dir(ctx).join("arch/x86/boot/bzImage"),
            rootfs_install: None, // copied directly by make_image, not the rootfs stage
        }]
    }

    fn install_mode(&self) -> InstallMode {
        InstallMode::StaticArtifacts
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A named, curated bundle of kernel Kconfig options — off by default,
/// turned on as a unit via `kernel.features`.
pub struct FeaturePack {
    pub key: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    options: &'static [&'static str],
}

pub const FEATURE_PACKS: &[FeaturePack] = &[
    FeaturePack {
        key: "graphics",
        label: "Graphics",
        description: "DRM/KMS graphics + fbdev console (i915, virtio-gpu, bochs, AGP) instead of plain VGA text",
        options: &[
            "CONFIG_DRM",
            "CONFIG_DRM_I915",
            "CONFIG_DRM_VIRTIO_GPU",
            "CONFIG_DRM_BOCHS",
            "CONFIG_DRM_FBDEV_EMULATION",
            "CONFIG_FB",
            "CONFIG_FRAMEBUFFER_CONSOLE",
            "CONFIG_AGP",
            "CONFIG_AGP_AMD64",
            "CONFIG_AGP_INTEL",
        ],
    },
    FeaturePack {
        key: "sound",
        label: "Sound",
        description: "ALSA sound subsystem and Intel HDA driver",
        options: &[
            "CONFIG_SOUND",
            "CONFIG_SND",
            "CONFIG_SND_HRTIMER",
            "CONFIG_SND_SEQUENCER",
            "CONFIG_SND_SEQ_DUMMY",
            "CONFIG_SND_HDA_INTEL",
            "CONFIG_SND_HDA_HWDEP",
        ],
    },
    FeaturePack {
        key: "wireless",
        label: "Wireless",
        description: "Wi-Fi stack (cfg80211/mac80211) and rfkill",
        options: &["CONFIG_CFG80211", "CONFIG_MAC80211", "CONFIG_MAC80211_LEDS", "CONFIG_RFKILL"],
    },
    FeaturePack {
        key: "hid-extras",
        label: "Vendor HID quirks",
        description: "Per-vendor HID quirk drivers (Sony, Samsung, Gyration, ...) and the hiddev/hidraw userspace interfaces; generic USB HID keyboards/mice work without this",
        options: &[
            "CONFIG_HID_GYRATION",
            "CONFIG_HID_NTRIG",
            "CONFIG_HID_PANTHERLORD",
            "CONFIG_PANTHERLORD_FF",
            "CONFIG_HID_PETALYNX",
            "CONFIG_HID_SAMSUNG",
            "CONFIG_HID_SONY",
            "CONFIG_HID_SUNPLUS",
            "CONFIG_HID_TOPSEED",
            "CONFIG_HID_PID",
            "CONFIG_HIDRAW",
            "CONFIG_USB_HIDDEV",
        ],
    },
    FeaturePack {
        key: "legacy-nics",
        label: "Legacy NIC drivers",
        description: "Dedicated Ethernet chipset drivers (Tigon3, Tulip, E100/E1000(E), Sky2, Forcedeth, 8139too, R8169) for real hardware; QEMU's virtio-net always works without this",
        options: &[
            "CONFIG_TIGON3",
            "CONFIG_NET_TULIP",
            "CONFIG_E100",
            "CONFIG_E1000",
            "CONFIG_E1000E",
            "CONFIG_SKY2",
            "CONFIG_FORCEDETH",
            "CONFIG_8139TOO",
            "CONFIG_R8169",
        ],
    },
    FeaturePack {
        key: "legacy-buses",
        label: "Legacy buses",
        description: "PCMCIA/CardBus (Yenta) and legacy PATA chipset drivers (AMD, old PIIX, SCH); AHCI/virtio-blk always work without this",
        options: &["CONFIG_PCCARD", "CONFIG_YENTA", "CONFIG_PATA_AMD", "CONFIG_PATA_OLDPIIX", "CONFIG_PATA_SCH"],
    },
    FeaturePack {
        key: "network-fs",
        label: "Network filesystems",
        description: "NFS (client + root-over-NFS), 9P, and autofs",
        options: &[
            "CONFIG_NFS_FS",
            "CONFIG_NFS_V3_ACL",
            "CONFIG_NFS_V4",
            "CONFIG_ROOT_NFS",
            "CONFIG_NET_9P",
            "CONFIG_NET_9P_VIRTIO",
            "CONFIG_AUTOFS_FS",
        ],
    },
    FeaturePack {
        key: "netfilter",
        label: "Netfilter/iptables",
        description: "Connection tracking, NAT, and iptables — only useful if this box routes or firewalls traffic",
        options: &[
            "CONFIG_NETFILTER",
            "CONFIG_NF_CONNTRACK",
            "CONFIG_NF_CONNTRACK_FTP",
            "CONFIG_NF_CONNTRACK_IRC",
            "CONFIG_NF_CONNTRACK_SIP",
            "CONFIG_NF_CT_NETLINK",
            "CONFIG_NF_NAT",
            "CONFIG_NETFILTER_XT_TARGET_CONNSECMARK",
            "CONFIG_NETFILTER_XT_TARGET_NFLOG",
            "CONFIG_NETFILTER_XT_TARGET_SECMARK",
            "CONFIG_NETFILTER_XT_TARGET_TCPMSS",
            "CONFIG_NETFILTER_XT_MATCH_CONNTRACK",
            "CONFIG_NETFILTER_XT_MATCH_POLICY",
            "CONFIG_NETFILTER_XT_MATCH_STATE",
            "CONFIG_IP_NF_IPTABLES",
            "CONFIG_IP_NF_FILTER",
            "CONFIG_IP_NF_TARGET_REJECT",
            "CONFIG_IP_NF_TARGET_MASQUERADE",
            "CONFIG_IP_NF_MANGLE",
            "CONFIG_IP6_NF_IPTABLES",
            "CONFIG_IP6_NF_MATCH_IPV6HEADER",
            "CONFIG_IP6_NF_FILTER",
            "CONFIG_IP6_NF_TARGET_REJECT",
            "CONFIG_IP6_NF_MANGLE",
        ],
    },
    FeaturePack {
        key: "security-extras",
        label: "Quota/ACL/SELinux",
        description: "Disk quotas, POSIX ACLs, and SELinux — irrelevant to a single-user BusyBox/coreutils rootfs",
        options: &[
            "CONFIG_QUOTA",
            "CONFIG_QUOTA_NETLINK_INTERFACE",
            "CONFIG_QFMT_V2",
            "CONFIG_EXT4_FS_POSIX_ACL",
            "CONFIG_EXT4_FS_SECURITY",
            "CONFIG_TMPFS_POSIX_ACL",
            "CONFIG_SECURITY_NETWORK",
            "CONFIG_SECURITY_SELINUX",
            "CONFIG_SECURITY_SELINUX_BOOTPARAM",
        ],
    },
    FeaturePack {
        key: "iommu",
        label: "IOMMU",
        description: "AMD/Intel IOMMU support — only needed for PCI passthrough or running this as a virtualization host",
        options: &["CONFIG_AMD_IOMMU", "CONFIG_INTEL_IOMMU"],
    },
    FeaturePack {
        key: "debug",
        label: "Debug/diagnostics",
        description: "Kernel debug instrumentation (Magic SysRq, schedstats, block IO tracing, boot-param/entry debug, early printk over USB debug port) — useful while bringing up boot, dead weight once stable",
        options: &[
            "CONFIG_DEBUG_KERNEL",
            "CONFIG_MAGIC_SYSRQ",
            "CONFIG_DEBUG_WX",
            "CONFIG_DEBUG_STACK_USAGE",
            "CONFIG_SCHEDSTATS",
            "CONFIG_BLK_DEV_IO_TRACE",
            "CONFIG_PROVIDE_OHCI1394_DMA_INIT",
            "CONFIG_EARLY_PRINTK_DBGP",
            "CONFIG_DEBUG_BOOT_PARAMS",
            "CONFIG_DEBUG_ENTRY",
            "CONFIG_DEBUG_DEVRES",
            "CONFIG_PM_DEBUG",
            "CONFIG_PM_TRACE_RTC",
        ],
    },
    FeaturePack {
        key: "ia32-emulation",
        label: "32-bit compat (IA32_EMULATION)",
        description: "Run 32-bit x86 binaries on this 64-bit kernel",
        options: &["CONFIG_IA32_EMULATION"],
    },
    FeaturePack {
        key: "iso9660",
        label: "ISO9660",
        description: "ISO9660/Joliet/zisofs filesystem support, for booting or mounting optical media images",
        options: &["CONFIG_ISO9660_FS", "CONFIG_JOLIET", "CONFIG_ZISOFS"],
    },
    FeaturePack {
        key: "boot-logo",
        label: "Boot logo",
        description: "Framebuffer console + boot-time Linux logo (penguin, or a custom image via kernel.logo_file)",
        options: &[
            "CONFIG_FB",
            "CONFIG_FB_EFI",
            "CONFIG_SYSFB_SIMPLEFB",
            "CONFIG_FRAMEBUFFER_CONSOLE",
            "CONFIG_LOGO",
            "CONFIG_LOGO_LINUX_CLUT224",
            "CONFIG_FB_LOGO_EXTRA",
        ],
    },
];

const LOGO_PATH: &str = "drivers/video/logo/logo_linux_clut224.ppm";

fn feature_pack(key: &str) -> Option<&'static FeaturePack> {
    FEATURE_PACKS.iter().find(|p| p.key == key)
}

fn disable_options(text: &mut String, options: &[&str]) {
    for option in options {
        for suffix in ["=y", "=m"] {
            let line = format!("{option}{suffix}");
            if text.contains(&line) {
                *text = text.replace(&line, &format!("# {option} is not set"));
            }
        }
    }
}

#[derive(Deserialize)]
struct ReleaseFeed {
    releases: Vec<Release>,
}

#[derive(Deserialize)]
struct Release {
    moniker: String,
    version: String,
    iseol: bool,
    source: Option<String>,
}

/// Looks up the current stable/LTS release from kernel.org's release feed.
/// Not tied to `Kernel`/`BuildCtx` — callers write the result into whatever
/// config shape they use.
pub fn latest_release(channel: KernelChannel) -> Result<(String, String)> {
    let moniker = match channel {
        KernelChannel::Stable => "stable",
        KernelChannel::Lts => "longterm",
    };

    println!("checking kernel.org for the current {moniker} release");
    let output = Command::new("wget")
        .args(["-qO-", "https://www.kernel.org/releases.json"])
        .output()
        .context("running wget")?;
    if !output.status.success() {
        bail!("wget failed: {}", String::from_utf8_lossy(&output.stderr));
    }

    let feed: ReleaseFeed =
        serde_json::from_slice(&output.stdout).context("parsing kernel.org's release feed")?;
    let release = feed
        .releases
        .into_iter()
        .filter(|r| r.moniker == moniker && !r.iseol && r.source.is_some())
        .max_by(|a, b| compare_versions(&a.version, &b.version))
        .with_context(|| format!("no active \"{moniker}\" release found in kernel.org's feed"))?;
    let source = release.source.expect("filtered to Some above");

    println!("using kernel {} ({source})", release.version);
    Ok((release.version, source))
}

#[derive(Clone, Copy)]
pub enum KernelChannel {
    Stable,
    Lts,
}

fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |s: &str| -> Vec<u32> {
        s.split(|c: char| !c.is_ascii_digit())
            .filter(|p| !p.is_empty())
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    parse(a).cmp(&parse(b))
}
