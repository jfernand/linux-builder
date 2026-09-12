use super::{already_built, run};
use crate::config::Config;
use crate::stages::toolchain::musl_target;
use anyhow::{Context, Result};
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::Command;

/// Utilities we expose from the uutils multi-call binary. Not exhaustive,
/// just enough for a usable minimal shell environment.
const COREUTILS_APPLETS: &[&str] = &[
    "ls", "cat", "cp", "mv", "rm", "mkdir", "rmdir", "echo", "pwd", "touch", "chmod", "chown",
    "ln", "grep", "sed", "head", "tail", "sort", "uniq", "wc", "find", "env", "true", "false",
    "test", "df", "du", "date", "uname", "sleep", "kill", "ps",
];

pub fn assemble_rootfs(cfg: &Config, force: bool) -> Result<()> {
    let root = cfg.rootfs_dir();

    if already_built(&root.join("etc/inittab"), force) {
        println!("skip assemble-rootfs: {} already assembled", root.display());
        return Ok(());
    }

    for dir in ["bin", "sbin", "etc", "proc", "sys", "dev", "lib", "usr/bin", "usr/sbin", "etc/init.d"] {
        fs::create_dir_all(root.join(dir))
            .with_context(|| format!("creating rootfs dir {dir}"))?;
    }

    install_coreutils(cfg, &root)?;
    install_busybox(cfg, &root)?;
    install_kernel_modules(cfg, &root)?;
    write_config_files(cfg, &root)?;

    Ok(())
}

fn install_coreutils(cfg: &Config, root: &Path) -> Result<()> {
    let src = cfg
        .uutils_build_dir()
        .join("target")
        .join(musl_target())
        .join("release")
        .join("coreutils");
    let dest = root.join("bin/coreutils");
    fs::copy(&src, &dest)
        .with_context(|| format!("copying {} to {}", src.display(), dest.display()))?;

    for applet in COREUTILS_APPLETS {
        let link = root.join("bin").join(applet);
        let _ = fs::remove_file(&link);
        symlink("coreutils", &link)
            .with_context(|| format!("symlinking bin/{applet} -> coreutils"))?;
    }

    Ok(())
}

/// Busybox applets referenced by /etc/inittab and /etc/init.d/rcS
/// (see BUSYBOX_APPLETS in build_busybox), exposed as /sbin/<name> symlinks.
const BUSYBOX_SBIN_APPLETS: &[&str] = &["hostname", "reboot", "poweroff", "halt", "swapoff"];

/// uutils/coreutils doesn't build a `mount`/`umount` applet under the
/// `feat_os_unix_musl` feature set, so route these through busybox instead.
const BUSYBOX_BIN_APPLETS: &[&str] = &["mount", "umount"];

fn install_busybox(cfg: &Config, root: &Path) -> Result<()> {
    let src = cfg.busybox_build_dir().join("busybox");
    let dest = root.join("bin/busybox");
    fs::copy(&src, &dest)
        .with_context(|| format!("copying {} to {}", src.display(), dest.display()))?;

    let sh_link = root.join("bin/sh");
    let _ = fs::remove_file(&sh_link);
    symlink("busybox", &sh_link).context("symlinking bin/sh -> busybox")?;

    for applet in BUSYBOX_BIN_APPLETS {
        let link = root.join("bin").join(applet);
        let _ = fs::remove_file(&link);
        symlink("busybox", &link).with_context(|| format!("symlinking bin/{applet} -> busybox"))?;
    }

    let init_link = root.join("sbin/init");
    let _ = fs::remove_file(&init_link);
    symlink("../bin/busybox", &init_link).context("symlinking sbin/init -> busybox")?;

    for applet in BUSYBOX_SBIN_APPLETS {
        let link = root.join("sbin").join(applet);
        let _ = fs::remove_file(&link);
        symlink("../bin/busybox", &link)
            .with_context(|| format!("symlinking sbin/{applet} -> busybox"))?;
    }

    Ok(())
}

fn install_kernel_modules(cfg: &Config, root: &Path) -> Result<()> {
    let kernel_dir = cfg.kernel_build_dir();
    let modules_dest = root.join("lib/modules");
    fs::create_dir_all(&modules_dest)?;

    run(Command::new("make")
        .current_dir(&kernel_dir)
        .arg(format!(
            "INSTALL_MOD_PATH={}",
            root.canonicalize().unwrap_or_else(|_| root.to_path_buf()).display()
        ))
        .arg("modules_install"))?;

    Ok(())
}

fn write_config_files(cfg: &Config, root: &Path) -> Result<()> {
    fs::write(root.join("etc/hostname"), format!("{}\n", cfg.image.hostname))?;

    fs::write(
        root.join("etc/fstab"),
        "proc  /proc  proc  defaults  0 0\nsysfs /sys   sysfs defaults  0 0\n",
    )?;

    fs::write(
        root.join("etc/inittab"),
        "::sysinit:/etc/init.d/rcS\n\
         ::respawn:/bin/sh\n\
         ::ctrlaltdel:/sbin/reboot\n\
         ::shutdown:/sbin/swapoff -a\n",
    )?;

    let rcs_path = root.join("etc/init.d/rcS");
    fs::write(
        &rcs_path,
        "#!/bin/sh\n\
         mount -t proc proc /proc\n\
         mount -t sysfs sysfs /sys\n\
         mount -t devtmpfs devtmpfs /dev 2>/dev/null\n\
         hostname -F /etc/hostname\n",
    )?;
    let mut perms = fs::metadata(&rcs_path)?.permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
    fs::set_permissions(&rcs_path, perms)?;

    Ok(())
}
