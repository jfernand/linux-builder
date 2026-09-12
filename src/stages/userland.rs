use super::{already_built, run_in};
use crate::config::Config;
use crate::stages::toolchain::musl_target;
use anyhow::Result;
use std::process::Command;

pub fn build_userland(cfg: &Config, force: bool) -> Result<()> {
    build_uutils(cfg, force)?;
    build_busybox(cfg, force)?;
    Ok(())
}

fn build_uutils(cfg: &Config, force: bool) -> Result<()> {
    let dir = cfg.uutils_build_dir();
    let target = musl_target();
    let binary = dir
        .join("target")
        .join(target)
        .join("release")
        .join("coreutils");

    if already_built(&binary, force) {
        println!("skip build-uutils: {} already exists", binary.display());
        return Ok(());
    }

    println!("building uutils/coreutils for {target}");
    run_in(
        &dir,
        Command::new("cargo").args([
            "build",
            "--release",
            "--target",
            target,
            "--no-default-features",
            "--features",
            "feat_os_unix_musl",
        ]),
    )?;

    Ok(())
}

/// We only need busybox for the shell (ash) and init/rc handling — the bulk
/// of userland comes from uutils. `defconfig` pulls in everything (including
/// networking applets like `tc` that don't compile against musl's minimal
/// uapi headers), so start from `allnoconfig` and enable just what we use.
const BUSYBOX_APPLETS: &[&str] = &[
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
];

/// musl-gcc's own include dir doesn't ship the `linux/*.h` uapi headers that
/// e.g. `init.c` needs (`linux/vt.h`); fall back to the system ones, which
/// are libc-agnostic, without letting them shadow musl's own headers.
const MUSL_CC: &str = "musl-gcc -idirafter /usr/include";

fn build_busybox(cfg: &Config, force: bool) -> Result<()> {
    let dir = cfg.busybox_build_dir();
    let binary = dir.join("busybox");

    if already_built(&binary, force) {
        println!("skip build-busybox: {} already exists", binary.display());
        return Ok(());
    }

    println!("configuring busybox (minimal, static, musl) in {}", dir.display());
    run_in(&dir, Command::new("make").arg("allnoconfig"))?;

    let config_path = dir.join(".config");
    let mut config = std::fs::read_to_string(&config_path)?;
    for applet in BUSYBOX_APPLETS {
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
    run_in(
        &dir,
        Command::new("sh").arg("-c").arg("yes '' | make oldconfig"),
    )?;

    println!("building busybox");
    // CC must be a make command-line variable, not an env var: busybox's
    // Makefile assigns `CC = $(CROSS_COMPILE)gcc` unconditionally, which
    // overrides (and silently shadows) an environment-provided CC.
    run_in(
        &dir,
        Command::new("make")
            .arg(format!("-j{}", num_cpus()))
            .arg(format!("CC={MUSL_CC}")),
    )?;

    Ok(())
}

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}
