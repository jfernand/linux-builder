//! BusyBox — `distroless`'s shell (ash) and init/rc handling; the bulk of
//! its userland comes from uutils instead. A Kconfig-based build, unlike
//! anything else in this crate: `allnoconfig`, patch `.config` to enable
//! a curated applet list, `oldconfig`, build with a make-time `CC`
//! override for musl.

use anyhow::Context;
use buildpack_core::run::{already_built, run_in};
use buildpack_core::{
    BuildCtx, BuildOutput, Buildpack, Description, InstallMode, RootfsInstall, Source,
};
use serde::Deserialize;
use std::any::Any;
use std::path::PathBuf;
use std::process::Command;

/// `defconfig` pulls in everything (including networking applets like
/// `tc` that don't compile against musl's minimal uapi headers), so start
/// from `allnoconfig` and enable just what's used.
const APPLETS: &[&str] = &[
    "STATIC",
    "ASH",
    "INIT",
    "FEATURE_USE_INITTAB",
    "MOUNT",
    "UMOUNT",
    "HOSTNAME",
    "SWAPOFF",
    "REBOOT",
    "POWEROFF",
    "HALT",
    "GETTY",
    "LOGIN",
    "PASSWD",
    // login's own crypt(3) call, not the system one: avoids pulling in
    // glibc's <crypt.h> (via the musl-header-fallback below), which drags
    // in glibc's <features.h>/<sys/cdefs.h> and conflicts with musl's.
    "USE_BB_CRYPT",
    "USE_BB_CRYPT_SHA",
];

/// Only built when `networking` is enabled (`BuildCtx::networking`,
/// toggleable from `distroless`'s TUI settings screen).
const NETWORKING_APPLETS: &[&str] =
    &["UDHCPC", "IFCONFIG", "FEATURE_IFCONFIG_STATUS", "ROUTE", "PING", "FEATURE_FANCY_PING"];

/// Busybox applets referenced by `/etc/inittab` and `/etc/init.d/rcS`,
/// exposed as `/sbin/<name>` symlinks (a different directory than the
/// binary itself, unlike everything else in `APPLETS`/`NETWORKING_BIN_APPLETS`
/// below).
const SBIN_APPLETS: &[&str] = &["hostname", "reboot", "poweroff", "halt", "swapoff", "getty"];

/// uutils/coreutils doesn't build a `mount`/`umount` applet under the
/// `feat_os_unix_musl` feature set, so these route through busybox
/// instead. `login` lives here too since that's where `getty` looks for
/// it by default (no `-l` override needed).
const BIN_APPLETS: &[&str] = &["mount", "umount", "login", "passwd"];

const NETWORKING_BIN_APPLETS: &[&str] = &["udhcpc", "ifconfig", "route", "ping"];

/// musl-gcc's own include dir doesn't ship the `linux/*.h`/`asm/*.h` uapi
/// headers that e.g. `init.c` (`linux/vt.h`) and the udhcpc networking
/// applets (`asm/types.h`, multiarch-pathed on Debian/Ubuntu) need; fall
/// back to the system ones, which are libc-agnostic, without letting them
/// shadow musl's own headers.
const MUSL_CC: &str = "musl-gcc -idirafter /usr/include -idirafter /usr/include/x86_64-linux-gnu";

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BusyboxConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Busybox {
    cfg: BusyboxConfig,
}

impl Busybox {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("busybox-{}", self.cfg.version))
    }
}

impl Buildpack for Busybox {
    fn id(&self) -> &'static str {
        "busybox"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [busybox] config")?;
        Ok(())
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    fn describe(&self) -> Description {
        Description {
            id: "busybox",
            name: "BusyBox",
            summary: "ash shell + init/rc handling — a curated minimal applet set",
            long_description: "allnoconfig, then a curated set of applets enabled by patching \
                .config directly (see APPLETS/NETWORKING_APPLETS), built static against musl.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("busybox-{}.tar.bz2", self.cfg.version),
            extracted_dir_name: format!("busybox-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let dir = self.build_dir(ctx);
        let binary = dir.join("busybox");

        if already_built(&binary, force) {
            println!("skip build-busybox: {} already exists", binary.display());
            return Ok(());
        }

        println!("configuring busybox (minimal, static, musl) in {}", dir.display());
        run_in(&dir, Command::new("make").arg("allnoconfig"))?;

        let mut applets = APPLETS.to_vec();
        if ctx.networking {
            applets.extend_from_slice(NETWORKING_APPLETS);
        }

        let config_path = dir.join(".config");
        let mut config = std::fs::read_to_string(&config_path)?;
        for applet in applets {
            let not_set = format!("# CONFIG_{applet} is not set");
            let enabled = format!("CONFIG_{applet}=y");
            if config.contains(&not_set) {
                config = config.replace(&not_set, &enabled);
            } else if !config.contains(&enabled) {
                config.push_str(&format!("{enabled}\n"));
            }
        }
        std::fs::write(&config_path, config)?;

        // Resolve dependent symbols (e.g. CONFIG_SH_IS_ASH) non-interactively,
        // keeping our explicit choices as the default answer at each prompt.
        run_in(&dir, Command::new("sh").arg("-c").arg("yes '' | make oldconfig"))?;

        println!("building busybox");
        // CC must be a make command-line variable, not an env var: busybox's
        // Makefile assigns `CC = $(CROSS_COMPILE)gcc` unconditionally, which
        // overrides (and silently shadows) an environment-provided CC.
        run_in(
            &dir,
            Command::new("make").arg(format!("-j{}", ctx.jobs)).arg(format!("CC={MUSL_CC}")),
        )
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        let dir = self.build_dir(ctx);

        let mut bin_applets = BIN_APPLETS.to_vec();
        if ctx.networking {
            bin_applets.extend_from_slice(NETWORKING_BIN_APPLETS);
        }

        let mut symlinks: Vec<PathBuf> = vec![PathBuf::from("bin/sh")];
        symlinks.extend(bin_applets.iter().map(|a| PathBuf::from("bin").join(a)));
        symlinks.push(PathBuf::from("sbin/init"));
        symlinks.extend(SBIN_APPLETS.iter().map(|a| PathBuf::from("sbin").join(a)));

        vec![BuildOutput {
            description: "busybox binary".to_string(),
            path: dir.join("busybox"),
            rootfs_install: Some(RootfsInstall { dest: PathBuf::from("bin/busybox"), symlinks }),
        }]
    }

    fn install_mode(&self) -> InstallMode {
        InstallMode::StaticArtifacts
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
