use crate::stages::buildpacks::install_static_outputs;
use anyhow::{Context, Result};
use buildpack_core::config::DistroConfig;
use buildpack_core::run::{already_built, run_in};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

/// Host system libraries our dynamically-linked binaries (dbus, seatd,
/// libinput, Mesa, ...) need at runtime but that our own build doesn't
/// produce — copied straight from the host, since we compile natively
/// against the host's own glibc (see config.rs's comments on "we are the
/// distro" via the host toolchain, not a cross one). `libgcc_s`/
/// `libstdc++` come from Mesa's C++ gallium code, `libz`/`libzstd` from
/// its compression use, `libffi` from libwayland-client's
/// wire-marshalling — the last of these was actually a latent Phase 2 gap
/// (libwayland-client has needed it since it was first built), just never
/// caught because nothing exercised it at runtime until Mesa's EGL now
/// links against it too. `libtinfo` is a fresh one from this same family:
/// Mesa's `lavapipe` Vulkan ICD statically links the host's own LLVM,
/// and that LLVM build was itself linked against `libtinfo` (terminal
/// color-support detection) — a dependency that only became reachable
/// once `-Dllvm=enabled` (added for Vulkan) actually got exercised at
/// runtime, the same "latent until something actually loads it" pattern
/// `libffi` hit above. Extend this list as later phases pull in more.
const HOST_DYNAMIC_LIBS: &[&str] = &[
    "libc.so.6",
    "libexpat.so.1",
    "libm.so.6",
    "libgcc_s.so.1",
    "libstdc++.so.6",
    "libz.so.1",
    "libzstd.so.1",
    "libffi.so.8",
    "libtinfo.so.6",
];
const HOST_LIB_DIR: &str = "/lib/x86_64-linux-gnu";
const HOST_DYNAMIC_LINKER: &str = "/lib64/ld-linux-x86-64.so.2";

/// Assembles the root filesystem tree: every `StaticArtifacts` buildpack's
/// declared outputs (uutils/coreutils, bash, util-linux's `agetty`/
/// `mount`/`umount`, shadow-utils' `login`/`passwd`, `distro-init` as
/// `/sbin/init`) copied in directly, the whole shared sysroot (every
/// `Sysroot`-mode buildpack) bulk-copied in one `cp -a`, plus the host
/// dynamic linker/libs and login config. `distro-init` mounts proc/sys/dev
/// itself and supervises `agetty` on the console — no BusyBox-style
/// `/etc/inittab`/`rcS` needed.
pub fn assemble_rootfs(cfg: &DistroConfig, force: bool) -> Result<()> {
    let root = cfg.rootfs_dir();

    if already_built(&root.join("sbin/init"), force) {
        println!("skip assemble-rootfs: {} already assembled", root.display());
        return Ok(());
    }

    for dir in [
        "bin", "sbin", "proc", "sys", "dev", "root", "etc", "tmp", "var/log", "var/run",
        // fontconfig's own fonts.conf (installed by the fontconfig
        // buildpack itself) declares this as its primary cache
        // directory — harmless while dejavu_fonts had zero real font
        // files to scan, but a real Alacritty boot test surfaced it the
        // moment fonts actually existed to cache: fontconfig's scan
        // failed with a bare, contextless `Os { code: 2, NotFound }`
        // once it had real work to do here.
        "var/cache/fontconfig",
    ] {
        fs::create_dir_all(root.join(dir)).with_context(|| format!("creating rootfs dir {dir}"))?;
    }

    install_static_outputs(cfg, &root)?;
    install_sysroot(cfg, &root)?;
    install_dynamic_linker_and_host_libs(&root)?;
    write_login_config(&root)?;
    write_bash_profile(&root)?;

    Ok(())
}

/// Every `Sysroot`-mode buildpack (seatd, dbus, eudev, wayland, and the
/// rest of the link-time libraries) gets built AND installed into
/// `cfg.sysroot_dir()` as part of `build-userland` itself — not just so
/// the final rootfs has them, but so each package's build can find an
/// *earlier* one (wayland-protocols needs `wayland-scanner`, libinput
/// needs eudev's `libudev.pc`) the way a real distro's build pipeline
/// chains packages through a sysroot rather than the host's own system
/// paths. Assembling the rootfs is then just copying that whole tree in:
/// `/usr/lib/x86_64-linux-gnu` is one of glibc's compiled-in default
/// dynamic-linker search paths on this (Ubuntu) host — confirmed via
/// `ld-linux-x86-64.so.2 --help` — so this needs no `ld.so.conf`/
/// `ldconfig` step for any of it to be found at runtime.
fn install_sysroot(cfg: &DistroConfig, root: &Path) -> Result<()> {
    let sysroot = std::env::current_dir().context("getting current directory")?.join(cfg.sysroot_dir());
    run_in(
        Path::new("."),
        Command::new("cp").arg("-a").arg(format!("{}/.", sysroot.display())).arg(root),
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

/// `/bin/login` execs the shell from `/etc/passwd` (`/bin/bash`) as a
/// login shell (`argv[0]` starting with `-`), which sources this file —
/// the one place to both (a) export the two Wayland-session variables
/// every interactive test in this project otherwise had to set by hand
/// (`XDG_RUNTIME_DIR`, `WAYLAND_DISPLAY` — matching `spawn_cosmic_comp`'s
/// own `XDG_RUNTIME_DIR` and smithay's `ListeningSocketSource::new_auto`
/// naming, §10.3.6.2), and (b) auto-start a terminal on tty1 specifically
/// — the one console QEMU's own window actually renders — since
/// `cosmic-comp` permanently takes DRM/KMS ownership away from tty1's
/// text console a few seconds into boot, so simply reaching a login
/// prompt there isn't enough to get a usable session visible in the
/// window on its own.
///
/// The wait loop exists because login can race `cosmic-comp`'s own
/// startup — its Wayland socket doesn't exist yet the instant tty1's
/// `agetty` shows a prompt. Falls through to a plain shell if it never
/// appears (cosmic-kiosk bundle disabled, or cosmic-comp crashed), same
/// as `ttyS0`, which this deliberately leaves untouched as a plain debug
/// shell — that's still the way to reach a shell without racing anything
/// or fighting the window for cosmic-term's own display.
///
/// Deliberately avoids external `tty`/`seq` — this project's `uutils`
/// build only symlinks a curated `COREUTILS_APPLETS` subset under
/// `/bin` (see `buildpacks/src/uutils.rs`), and neither is in it (the
/// underlying binary supports both, just unreached without a matching
/// argv0). `[ path -ef path ]` (same underlying file, i.e. same tty) and
/// a plain counted `while` loop are pure bash builtins, so this doesn't
/// silently break the next time that applet list changes.
///
/// Deliberately does NOT `exec cosmic-term`: unlike Alacritty,
/// `cosmic-term` daemonizes itself (§10.3.6.6 — it has a real `fork`
/// dependency), so its own top-level process always exits almost
/// immediately once it's forked off the real, detached worker. `exec`ing
/// into it — first tried here — meant *that exit* took the whole
/// `agetty`→`login`→`bash` chain down with it (no fork ever created a
/// new process to exit independently), which `distro-init` then
/// respawned, autologin and all, launching yet another `cosmic-term`
/// forever — five-plus stacked instances confirmed in under 8 seconds
/// of boot. Running it plain (no `exec`) lets its own daemonizing exit
/// return control to *this* script instead of unwinding the login chain;
/// `break` out of the wait loop and fall through to an ordinary (idle,
/// invisible once cosmic-comp owns DRM, but alive) interactive shell,
/// which is all that's needed to stop the respawn loop.
fn write_bash_profile(root: &Path) -> Result<()> {
    fs::write(
        root.join("root/.bash_profile"),
        "\
export XDG_RUNTIME_DIR=/run/user/0
export WAYLAND_DISPLAY=wayland-1

if [ /proc/self/fd/0 -ef /dev/tty1 ]; then
    i=0
    while [ $i -lt 50 ]; do
        if [ -S \"$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY\" ]; then
            cosmic-term
            break
        fi
        sleep 0.2
        i=$((i + 1))
    done
fi
",
    )?;
    Ok(())
}
