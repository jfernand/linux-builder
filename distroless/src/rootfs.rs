//! Assembles the root filesystem: `busybox`/`uutils`' declared outputs
//! (via `crate::pipeline::install_static_outputs`), the kernel modules
//! this build produced, and the config files (`/etc/inittab`, `/etc/
//! passwd`, `/etc/fstab`, `/etc/init.d/rcS`, optionally the udhcpc
//! script) that aren't any single package's concern.

use crate::pipeline::install_static_outputs;
use anyhow::{Context, Result};
use builder_core::config::Config;
use builder_core::stages::already_built;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn assemble_rootfs(config_path: &Path, cfg: &Config, force: bool) -> Result<()> {
    let root = cfg.rootfs_dir();

    if already_built(&root.join("etc/inittab"), force) {
        println!("skip assemble-rootfs: {} already assembled", root.display());
        return Ok(());
    }

    for dir in [
        "bin", "sbin", "etc", "proc", "sys", "dev", "lib", "usr/bin", "usr/sbin", "etc/init.d", "root",
    ] {
        fs::create_dir_all(root.join(dir)).with_context(|| format!("creating rootfs dir {dir}"))?;
    }

    install_static_outputs(config_path, cfg, &root)?;
    install_kernel_modules(cfg, &root)?;
    write_config_files(cfg, &root)?;
    if cfg.networking {
        write_udhcpc_script(&root)?;
    }

    Ok(())
}

fn install_kernel_modules(cfg: &Config, root: &Path) -> Result<()> {
    let kernel_dir = cfg.build_dir.join("kernel").join(format!("linux-{}", cfg.kernel.version));
    let modules_dest = root.join("lib/modules");
    fs::create_dir_all(&modules_dest)?;

    builder_core::stages::run(
        Command::new("make")
            .current_dir(&kernel_dir)
            .arg(format!(
                "INSTALL_MOD_PATH={}",
                root.canonicalize().unwrap_or_else(|_| root.to_path_buf()).display()
            ))
            .arg("modules_install"),
    )
}

fn write_config_files(cfg: &Config, root: &Path) -> Result<()> {
    fs::write(root.join("etc/hostname"), format!("{}\n", cfg.image.hostname))?;

    fs::write(
        root.join("etc/fstab"),
        "proc  /proc  proc  defaults  0 0\nsysfs /sys   sysfs defaults  0 0\n",
    )?;

    // A single root account with no password. BusyBox `login` only takes
    // its no-password shortcut when /etc/passwd's own password field is
    // empty (`pw->pw_passwd[0] == 0`); an "x" + a shadow entry routes
    // through crypt() instead, which rejects an empty hash as a bad salt.
    // So: no /etc/shadow, empty field directly in /etc/passwd.
    fs::write(root.join("etc/passwd"), "root::0:0:root:/root:/bin/sh\n")?;

    fs::write(
        root.join("etc/inittab"),
        "::sysinit:/etc/init.d/rcS\n\
         tty1::respawn:/sbin/getty 38400 tty1\n\
         ttyS0::respawn:/sbin/getty -L 115200 ttyS0 vt100\n\
         ::ctrlaltdel:/sbin/reboot\n\
         ::shutdown:/sbin/swapoff -a\n",
    )?;

    let networking_lines = if cfg.networking {
        "ifconfig lo 127.0.0.1 up\n\
         udhcpc -i eth0 -s /usr/share/udhcpc/default.script -b\n"
    } else {
        ""
    };

    let rcs_path = root.join("etc/init.d/rcS");
    fs::write(
        &rcs_path,
        format!(
            "#!/bin/sh\n\
             mount -t proc proc /proc\n\
             mount -t sysfs sysfs /sys\n\
             mount -t devtmpfs devtmpfs /dev 2>/dev/null\n\
             hostname -F /etc/hostname\n\
             {networking_lines}"
        ),
    )?;
    make_executable(&rcs_path)?;

    Ok(())
}

/// Busybox's standard udhcpc bound/renew/deconfig handler: applies the
/// leased address via `ifconfig`, replaces the default route, and writes
/// /etc/resolv.conf.
fn write_udhcpc_script(root: &Path) -> Result<()> {
    let dir = root.join("usr/share/udhcpc");
    fs::create_dir_all(&dir)?;

    let script_path: PathBuf = dir.join("default.script");
    fs::write(
        &script_path,
        "#!/bin/sh\n\
         RESOLV_CONF=\"/etc/resolv.conf\"\n\
         case \"$1\" in\n\
         \tdeconfig)\n\
         \t\tifconfig \"$interface\" 0.0.0.0\n\
         \t\t;;\n\
         \trenew|bound)\n\
         \t\tifconfig \"$interface\" \"$ip\" ${subnet:+netmask \"$subnet\"} ${broadcast:+broadcast \"$broadcast\"}\n\
         \t\tif [ -n \"$router\" ]; then\n\
         \t\t\twhile route del default gw 0.0.0.0 dev \"$interface\" 2>/dev/null; do :; done\n\
         \t\t\tfor i in $router; do route add default gw \"$i\" dev \"$interface\"; done\n\
         \t\tfi\n\
         \t\t> \"$RESOLV_CONF\"\n\
         \t\t[ -n \"$domain\" ] && echo \"search $domain\" >> \"$RESOLV_CONF\"\n\
         \t\tfor i in $dns; do echo \"nameserver $i\" >> \"$RESOLV_CONF\"; done\n\
         \t\t;;\n\
         esac\n\
         exit 0\n",
    )?;
    make_executable(&script_path)?;

    Ok(())
}

fn make_executable(path: &Path) -> Result<()> {
    let mut perms = fs::metadata(path)?.permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
    fs::set_permissions(path, perms)?;
    Ok(())
}
