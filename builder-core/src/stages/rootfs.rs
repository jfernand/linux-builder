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
    "test", "[", "df", "du", "date", "uname", "sleep", "kill", "ps",
];

pub fn assemble_rootfs(cfg: &Config, force: bool) -> Result<()> {
    let root = cfg.rootfs_dir();

    if already_built(&root.join("etc/inittab"), force) {
        println!("skip assemble-rootfs: {} already assembled", root.display());
        return Ok(());
    }

    for dir in ["bin", "sbin", "etc", "proc", "sys", "dev", "lib", "usr/bin", "usr/sbin", "etc/init.d", "root"] {
        fs::create_dir_all(root.join(dir))
            .with_context(|| format!("creating rootfs dir {dir}"))?;
    }

    install_coreutils(cfg, &root)?;
    install_busybox(cfg, &root)?;
    install_kernel_modules(cfg, &root)?;
    write_config_files(cfg, &root)?;
    if cfg.networking {
        write_udhcpc_script(&root)?;
    }

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
const BUSYBOX_SBIN_APPLETS: &[&str] = &["hostname", "reboot", "poweroff", "halt", "swapoff", "getty"];

/// uutils/coreutils doesn't build a `mount`/`umount` applet under the
/// `feat_os_unix_musl` feature set, so route these through busybox instead.
/// `login` lives here too since that's where `getty` looks for it by
/// default (no `-l` override needed).
const BUSYBOX_BIN_APPLETS: &[&str] = &["mount", "umount", "login", "passwd"];

/// Only symlinked when `networking` is enabled (see BUSYBOX_NETWORKING_APPLETS
/// in build_busybox, which is what actually compiles these applets in).
const BUSYBOX_NETWORKING_BIN_APPLETS: &[&str] = &["udhcpc", "ifconfig", "route", "ping"];

fn install_busybox(cfg: &Config, root: &Path) -> Result<()> {
    let src = cfg.busybox_build_dir().join("busybox");
    let dest = root.join("bin/busybox");
    fs::copy(&src, &dest)
        .with_context(|| format!("copying {} to {}", src.display(), dest.display()))?;

    let sh_link = root.join("bin/sh");
    let _ = fs::remove_file(&sh_link);
    symlink("busybox", &sh_link).context("symlinking bin/sh -> busybox")?;

    let mut bin_applets = BUSYBOX_BIN_APPLETS.to_vec();
    if cfg.networking {
        bin_applets.extend_from_slice(BUSYBOX_NETWORKING_BIN_APPLETS);
    }
    for applet in bin_applets {
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

    // A single root account with no password. BusyBox `login` only takes
    // its no-password shortcut when /etc/passwd's own password field is
    // empty (`pw->pw_passwd[0] == 0`); an "x" + a shadow entry routes
    // through crypt() instead, which rejects an empty hash as a bad salt.
    // So: no /etc/shadow, empty field directly in /etc/passwd. `login`
    // still prompts for a password, but any input (including none) is
    // accepted — you get a real login prompt without needing to bake in or
    // remember a password. Set one (`passwd`, once logged in) before
    // exposing this to a network.
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
/// /etc/resolv.conf. udhcpc runs this itself on lease events; it doesn't
/// configure anything on its own.
fn write_udhcpc_script(root: &Path) -> Result<()> {
    let dir = root.join("usr/share/udhcpc");
    fs::create_dir_all(&dir)?;

    let script_path = dir.join("default.script");
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
