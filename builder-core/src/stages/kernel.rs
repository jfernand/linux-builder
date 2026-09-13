use super::{already_built, run_in};
use crate::config::Config;
use anyhow::{bail, Context, Result};
use clap::ValueEnum;
use serde::Deserialize;
use std::path::Path;
use std::process::Command;

#[derive(Clone, Copy, ValueEnum)]
pub enum KernelChannel {
    /// The current mainline stable release
    Stable,
    /// The newest maintained long-term-support branch
    Lts,
}

pub fn build_kernel(cfg: &Config, force: bool) -> Result<()> {
    let dir = cfg.kernel_build_dir();
    let bzimage = dir.join("arch/x86/boot/bzImage");

    if already_built(&bzimage, force) {
        println!("skip build-kernel: {} already exists", bzimage.display());
        return Ok(());
    }

    configure_kernel(cfg, &dir)?;
    apply_custom_logo(cfg, &dir)?;

    let jobs = num_cpus();
    println!("building kernel ({jobs} jobs)");
    run_in(
        &dir,
        Command::new("make").arg(format!("-j{jobs}")),
    )?;

    Ok(())
}

/// Seeds and finalizes the kernel's `.config`.
///
/// Without `kernel.config_file`: `defconfig`, then stripped down to a
/// minimal base by turning off every option owned by a `FEATURE_PACKS`
/// entry (sound, wireless, legacy drivers, etc. — all "typical desktop"
/// bloat this project doesn't need), before re-enabling whichever packs
/// `kernel.features` names. This mirrors BusyBox's `allnoconfig` + curated
/// applet list in `userland.rs`, but starting from `defconfig` rather than
/// `allnoconfig` since reconstructing x86_64 boot essentials (PCI, ACPI,
/// EFI, block/ATA/virtio, ext4/vfat, console, ...) from nothing is fragile;
/// everything that isn't a named pack option is left exactly as `defconfig`
/// set it.
///
/// With `kernel.config_file`: that saved config is used as-is (only
/// re-resolved via `olddefconfig`, in case it predates a kernel upgrade) —
/// it's what the user hand-picked in `menu-config`, so we don't second-guess
/// it by shrinking. Feature packs still layer on top, in case the user
/// wants named toggles on top of their custom base too.
fn configure_kernel(cfg: &Config, dir: &Path) -> Result<()> {
    let config_path = dir.join(".config");
    let shrink_to_base = match &cfg.kernel.config_file {
        Some(path) => {
            println!("applying custom kernel config from {}", path.display());
            std::fs::copy(path, &config_path).with_context(|| {
                format!("copying {} into {}", path.display(), dir.display())
            })?;
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
    enable_features(cfg, &mut text)?;

    std::fs::write(&config_path, text)?;
    run_in(dir, Command::new("make").arg("olddefconfig"))?;
    Ok(())
}

/// A named, curated bundle of kernel Kconfig options — off by default (see
/// `configure_kernel`), turned on as a unit via `kernel.features` in the
/// config file or the TUI settings screen, so users can enable e.g.
/// "Graphics" without knowing the underlying CONFIG_* symbols.
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
        options: &[
            "CONFIG_CFG80211",
            "CONFIG_MAC80211",
            "CONFIG_MAC80211_LEDS",
            "CONFIG_RFKILL",
        ],
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
        options: &[
            "CONFIG_PCCARD",
            "CONFIG_YENTA",
            "CONFIG_PATA_AMD",
            "CONFIG_PATA_OLDPIIX",
            "CONFIG_PATA_SCH",
        ],
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

/// Path to the stock 80x80, 224-color boot logo within the kernel source
/// tree, so it can be swapped for `kernel.logo_file`.
const LOGO_PATH: &str = "drivers/video/logo/logo_linux_clut224.ppm";

/// Replaces the stock boot logo with `kernel.logo_file`, if both it and the
/// `boot-logo` feature are set. The kernel's own logo converter (run during
/// the build) rejects anything that isn't an 80x80 ASCII (P3) PPM with at
/// most 224 distinct colors, so we don't re-validate the format here.
fn apply_custom_logo(cfg: &Config, dir: &Path) -> Result<()> {
    let Some(logo_file) = &cfg.kernel.logo_file else {
        return Ok(());
    };
    if !cfg.kernel.features.iter().any(|f| f == "boot-logo") {
        return Ok(());
    }
    println!("using custom boot logo from {}", logo_file.display());
    std::fs::copy(logo_file, dir.join(LOGO_PATH))
        .with_context(|| format!("copying {} into {}", logo_file.display(), dir.display()))?;
    Ok(())
}

fn feature_pack(key: &str) -> Option<&'static FeaturePack> {
    FEATURE_PACKS.iter().find(|p| p.key == key)
}

/// Turns off every option in `options` that's currently on (`=y` or `=m`).
/// Options `defconfig` never turned on in the first place, and thus don't
/// even appear as an explicit `# ... is not set` line, are already off and
/// left untouched.
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

/// Turns on every option named by `kernel.features`.
fn enable_features(cfg: &Config, text: &mut String) -> Result<()> {
    for key in &cfg.kernel.features {
        let pack = feature_pack(key).with_context(|| {
            format!("unknown kernel feature \"{key}\" (see `linux-builder list-features`)")
        })?;
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

/// Interactively runs `make menuconfig` against the kernel sources so
/// options can be toggled by hand, then saves the resulting `.config` to
/// `save_to` for reuse as `kernel.config_file`.
pub fn menuconfig(cfg: &Config, save_to: &Path) -> Result<()> {
    let dir = cfg.kernel_build_dir();
    if !dir.exists() {
        bail!(
            "kernel sources not found at {} — run `fetch` first",
            dir.display()
        );
    }

    if !dir.join(".config").exists() {
        configure_kernel(cfg, &dir)?;
    }

    run_in(&dir, Command::new("make").arg("menuconfig"))?;

    if let Some(parent) = save_to.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(dir.join(".config"), save_to)
        .with_context(|| format!("saving kernel config to {}", save_to.display()))?;

    println!("saved kernel config to {}", save_to.display());
    println!(
        "set kernel.config_file = \"{}\" in your config file to build with it",
        save_to.display()
    );

    Ok(())
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

/// Looks up the current stable or long-term-support release from
/// kernel.org's release feed and writes its version/url into the config
/// file's `[kernel]` section, so `fetch`/`build-kernel` pick it up as-is.
pub fn resolve_kernel(config_path: &Path, channel: KernelChannel) -> Result<()> {
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

    let feed: ReleaseFeed = serde_json::from_slice(&output.stdout)
        .context("parsing kernel.org's release feed")?;
    let release = feed
        .releases
        .into_iter()
        .filter(|r| r.moniker == moniker && !r.iseol && r.source.is_some())
        .max_by(|a, b| compare_versions(&a.version, &b.version))
        .with_context(|| format!("no active \"{moniker}\" release found in kernel.org's feed"))?;
    let source = release.source.expect("filtered to Some above");

    println!("using kernel {} ({source})", release.version);

    let mut cfg = Config::load(config_path)?;
    cfg.kernel.version = release.version;
    cfg.kernel.url = source;
    cfg.save(config_path)?;

    println!("wrote kernel.version/url to {}", config_path.display());
    println!("if you already fetched a different version's sources, run `fetch --clean` to redo it");
    Ok(())
}

/// Compares dotted version strings (e.g. "6.12.109") component-wise, as
/// integers rather than lexicographically ("6.9" < "6.12").
fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |s: &str| -> Vec<u32> {
        s.split(|c: char| !c.is_ascii_digit())
            .filter(|p| !p.is_empty())
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    parse(a).cmp(&parse(b))
}

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}
