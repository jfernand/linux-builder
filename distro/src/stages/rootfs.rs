use crate::config::Config;
use crate::stages::userland::{
    agetty_binary_path, init_binary_path, mount_binary_path, shadow_binary_path,
    umount_binary_path, uutils_binary_path,
};
use anyhow::{Context, Result};
use builder_core::stages::{already_built, run_in};
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::Path;
use std::process::Command;

/// Host system libraries our first dynamically-linked binaries (dbus,
/// seatd) need at runtime but that our own build doesn't produce —
/// copied straight from the host, since we compile natively against the
/// host's own glibc (see the config.rs/userland.rs comments on "we are
/// the distro" via the host toolchain, not a cross one). Extend this list
/// as later phases (Wayland, libinput, Mesa, ...) pull in more of them.
const HOST_DYNAMIC_LIBS: &[&str] = &["libc.so.6", "libexpat.so.1"];
const HOST_LIB_DIR: &str = "/lib/x86_64-linux-gnu";
const HOST_DYNAMIC_LINKER: &str = "/lib64/ld-linux-x86-64.so.2";

/// Utilities exposed from the uutils multi-call binary. Not exhaustive,
/// just enough for a usable minimal shell environment — same starting
/// list `distroless` uses, since uutils supports the same applets
/// regardless of target libc.
const COREUTILS_APPLETS: &[&str] = &[
    "ls", "cat", "cp", "mv", "rm", "mkdir", "rmdir", "echo", "pwd", "touch", "chmod", "chown",
    "ln", "grep", "sed", "head", "tail", "sort", "uniq", "wc", "find", "env", "true", "false",
    "test", "[", "df", "du", "date", "uname", "sleep", "kill", "ps",
];

/// Assembles the root filesystem tree: uutils/coreutils, bash, util-linux's
/// `agetty`/`mount`/`umount`, shadow-utils' `login`/`passwd`, and our own
/// `distro-init` as `/sbin/init`. `distro-init` mounts proc/sys/dev itself
/// and supervises `agetty` on the console — no BusyBox-style
/// `/etc/inittab`/`rcS` needed.
pub fn assemble_rootfs(cfg: &Config, force: bool) -> Result<()> {
    let root = cfg.rootfs_dir();

    if already_built(&root.join("sbin/init"), force) {
        println!("skip assemble-rootfs: {} already assembled", root.display());
        return Ok(());
    }

    for dir in [
        "bin", "sbin", "proc", "sys", "dev", "root", "etc", "var/log", "var/run",
    ] {
        fs::create_dir_all(root.join(dir)).with_context(|| format!("creating rootfs dir {dir}"))?;
    }

    install_coreutils(cfg, &root)?;
    install_bash(cfg, &root)?;
    install_util_linux(cfg, &root)?;
    install_shadow(cfg, &root)?;
    install_seatd(cfg, &root)?;
    install_dbus(cfg, &root)?;
    install_eudev(cfg, &root)?;
    install_dynamic_linker_and_host_libs(&root)?;
    install_init(&root)?;
    write_login_config(&root)?;

    Ok(())
}

fn install_coreutils(cfg: &Config, root: &Path) -> Result<()> {
    let src = uutils_binary_path(cfg);
    let dest = root.join("bin/coreutils");
    fs::copy(&src, &dest).with_context(|| format!("copying {} to {}", src.display(), dest.display()))?;

    for applet in COREUTILS_APPLETS {
        let link = root.join("bin").join(applet);
        let _ = fs::remove_file(&link);
        symlink("coreutils", &link).with_context(|| format!("symlinking bin/{applet} -> coreutils"))?;
    }

    Ok(())
}

fn install_bash(cfg: &Config, root: &Path) -> Result<()> {
    let src = cfg.bash_build_dir().join("bash");
    let dest = root.join("bin/bash");
    fs::copy(&src, &dest).with_context(|| format!("copying {} to {}", src.display(), dest.display()))?;

    let sh_link = root.join("bin/sh");
    let _ = fs::remove_file(&sh_link);
    symlink("bash", &sh_link).context("symlinking bin/sh -> bash")?;

    Ok(())
}

fn install_util_linux(cfg: &Config, root: &Path) -> Result<()> {
    copy_binary(&agetty_binary_path(cfg), &root.join("sbin/agetty"))?;
    copy_binary(&mount_binary_path(cfg), &root.join("bin/mount"))?;
    copy_binary(&umount_binary_path(cfg), &root.join("bin/umount"))?;
    Ok(())
}

fn install_shadow(cfg: &Config, root: &Path) -> Result<()> {
    copy_binary(&shadow_binary_path(cfg, "login"), &root.join("bin/login"))?;
    copy_binary(&shadow_binary_path(cfg, "passwd"), &root.join("bin/passwd"))?;
    Ok(())
}

/// seatd and dbus are meson projects, unlike the autotools/uutils tools
/// above — rather than hand-picking files to copy, `ninja install` with
/// `DESTDIR` set to the rootfs does the same install meson would do onto
/// a real system (binaries under `/usr/bin`, dbus's own `libdbus-1.so.3`
/// under `/usr/lib/x86_64-linux-gnu`, dbus's `/etc/dbus-1/*.conf`, ...).
/// `/usr/lib/x86_64-linux-gnu` is one of glibc's compiled-in default
/// dynamic-linker search paths on this (Ubuntu) host — confirmed via
/// `ld-linux-x86-64.so.2 --help` — so this needs no `ld.so.conf`/
/// `ldconfig` step to be found at runtime.
fn ninja_install(build_dir: &Path, root: &Path) -> Result<()> {
    let destdir = std::env::current_dir().context("getting current directory")?.join(root);
    run_in(
        build_dir,
        Command::new("ninja").arg("install").env("DESTDIR", destdir),
    )
}

fn install_seatd(cfg: &Config, root: &Path) -> Result<()> {
    ninja_install(&cfg.seatd_build_dir().join("build"), root)
}

fn install_dbus(cfg: &Config, root: &Path) -> Result<()> {
    ninja_install(&cfg.dbus_build_dir().join("build"), root)
}

/// eudev is autotools, not meson, but the same DESTDIR trick applies —
/// `make install` with DESTDIR set to the rootfs installs udevd, libudev,
/// and the udev rules/hwdb data files exactly where Phase 3's libinput
/// will expect to find them.
fn install_eudev(cfg: &Config, root: &Path) -> Result<()> {
    let destdir = std::env::current_dir().context("getting current directory")?.join(root);
    run_in(
        &cfg.eudev_build_dir(),
        Command::new("make").arg("install").env("DESTDIR", destdir),
    )
}

fn install_dynamic_linker_and_host_libs(root: &Path) -> Result<()> {
    let lib64 = root.join("lib64");
    fs::create_dir_all(&lib64).context("creating rootfs dir lib64")?;
    copy_binary(
        Path::new(HOST_DYNAMIC_LINKER),
        &lib64.join("ld-linux-x86-64.so.2"),
    )?;

    let lib_dir = root.join(&HOST_LIB_DIR[1..]);
    fs::create_dir_all(&lib_dir).with_context(|| format!("creating rootfs dir {}", lib_dir.display()))?;
    for name in HOST_DYNAMIC_LIBS {
        let src = Path::new(HOST_LIB_DIR).join(name);
        copy_binary(&src, &lib_dir.join(name))?;
    }

    Ok(())
}

fn install_init(root: &Path) -> Result<()> {
    copy_binary(&init_binary_path(), &root.join("sbin/init"))
}

fn copy_binary(src: &Path, dest: &Path) -> Result<()> {
    fs::copy(src, dest).with_context(|| format!("copying {} to {}", src.display(), dest.display()))?;
    Ok(())
}

/// A single passwordless `root` account (empty field in `/etc/shadow` —
/// `login` still prompts for a password, but accepts any input including
/// none; run `passwd` once logged in to set a real one), plus the handful
/// of files shadow-utils' `login` expects to exist (even if empty) so it
/// doesn't warn about a missing login-record/database.
fn write_login_config(root: &Path) -> Result<()> {
    fs::write(root.join("etc/passwd"), "root:x:0:0:root:/root:/bin/bash\n")?;
    fs::write(root.join("etc/group"), "root:x:0:\n")?;

    let shadow_path = root.join("etc/shadow");
    fs::write(&shadow_path, "root::19999:0:99999:7:::\n")?;
    fs::set_permissions(&shadow_path, fs::Permissions::from_mode(0o600))?;

    let gshadow_path = root.join("etc/gshadow");
    fs::write(&gshadow_path, "root:::\n")?;
    fs::set_permissions(&gshadow_path, fs::Permissions::from_mode(0o600))?;

    for f in ["var/log/lastlog", "var/log/wtmp", "var/run/utmp"] {
        fs::write(root.join(f), [])?;
    }

    Ok(())
}
