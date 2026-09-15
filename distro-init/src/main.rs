//! `distro`'s own PID 1: mounts the basic virtual filesystems, starts
//! udevd/seatd/dbus (Phase 2's device-management and seat/session
//! plumbing), `cosmic-comp` (COSMIC's compositor, §10.3.6 of the phase
//! report) once devices have settled, and supervises `agetty` on both
//! the VGA console and the serial console — respawning any of the five
//! services if it exits and reaping any other orphaned children.
//! `agetty` execs `/bin/login` (shadow-utils) once a username is
//! entered, which authenticates against `/etc/passwd`/`/etc/shadow` and
//! execs the user's shell — this replaces Phase 1a's direct shell spawn.

use nix::mount::{mount, MsFlags};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::{execv, fork, ForkResult, Pid};
use std::ffi::CString;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

const AGETTY: &str = "/sbin/agetty";
const SEATD: &str = "/usr/bin/seatd";
const DBUS_DAEMON: &str = "/usr/bin/dbus-daemon";
const UDEVD: &str = "/usr/sbin/udevd";
const UDEVADM: &str = "/usr/bin/udevadm";
const COSMIC_COMP: &str = "/usr/bin/cosmic-comp";
const XDG_RUNTIME_DIR: &str = "/run/user/0";

fn mount_basic_filesystems() {
    // Best-effort: devtmpfs is often already mounted by the kernel itself
    // (CONFIG_DEVTMPFS_MOUNT) before init even runs.
    let _ = mount(Some("proc"), "/proc", Some("proc"), MsFlags::empty(), None::<&str>);
    let _ = mount(Some("sysfs"), "/sys", Some("sysfs"), MsFlags::empty(), None::<&str>);
    let _ = mount(Some("devtmpfs"), "/dev", Some("devtmpfs"), MsFlags::empty(), None::<&str>);
    // /run holds transient runtime state (seatd's and dbus's sockets among
    // it) and is expected to be a tmpfs, not part of the persisted rootfs.
    let _ = mount(Some("tmpfs"), "/run", Some("tmpfs"), MsFlags::empty(), None::<&str>);
    // dbus-daemon expects this directory to already exist for its system
    // bus socket; nothing else creates it now that /run is a fresh tmpfs.
    let _ = std::fs::create_dir("/run/dbus");
    // wayland-server (via cosmic-comp) refuses to create its socket
    // unless XDG_RUNTIME_DIR exists with exactly these permissions —
    // 0700, since it's meant to be private to the user running it.
    let _ = std::fs::create_dir_all(XDG_RUNTIME_DIR);
    let _ = std::fs::set_permissions(XDG_RUNTIME_DIR, std::fs::Permissions::from_mode(0o700));
}

fn spawn(path: &str, args: &[&str]) -> Pid {
    spawn_env(path, args, &[])
}

fn spawn_env(path: &str, args: &[&str], envs: &[(&str, &str)]) -> Pid {
    match unsafe { fork() } {
        Ok(ForkResult::Parent { child, .. }) => child,
        Ok(ForkResult::Child) => {
            // Each fork gets its own copy of the environment, so this
            // never leaks into distro-init's own or any sibling's env.
            // Safe here: this child is single-threaded (just forked, and
            // nothing between fork() and this loop spawns another
            // thread), which is the actual hazard set_var's unsafety
            // warns about.
            for (k, v) in envs {
                unsafe { std::env::set_var(k, v) };
            }
            let c_path = CString::new(path).unwrap();
            let c_args: Vec<CString> = args.iter().map(|a| CString::new(*a).unwrap()).collect();
            let _ = execv(&c_path, &c_args);
            // Only reached if exec itself failed (e.g. the binary is missing).
            std::process::exit(127);
        }
        Err(e) => panic!("distro-init: fork failed: {e}"),
    }
}

fn spawn_tty1() -> Pid {
    spawn(AGETTY, &[AGETTY, "38400", "tty1"])
}

fn spawn_serial() -> Pid {
    // -L: local line, skip waiting for carrier-detect (needed for QEMU's
    // virtual serial port, which never asserts one).
    spawn(AGETTY, &[AGETTY, "-L", "115200", "ttyS0", "vt100"])
}

fn spawn_seatd() -> Pid {
    spawn(SEATD, &[SEATD])
}

fn spawn_dbus() -> Pid {
    // --nofork: dbus-daemon daemonizes (forks and exits the parent) by
    // default, which would make it invisible to our own fork/waitpid
    // supervision below.
    spawn(DBUS_DAEMON, &[DBUS_DAEMON, "--system", "--nofork"])
}

fn spawn_udevd() -> Pid {
    // No -d: same reasoning as dbus-daemon's --nofork above.
    spawn(UDEVD, &[UDEVD])
}

fn spawn_cosmic_comp() -> Pid {
    // No WAYLAND_DISPLAY/DISPLAY/COSMIC_BACKEND set, so cosmic-comp's own
    // init_backend_auto() falls through to its "kms" backend — the real
    // DRM/libseat/udev path, not winit/x11. It finds seatd via seatd's
    // own default socket path (no SEATD_SOCK override needed, since
    // spawn_seatd() above never passed one either). HOME=/root matches
    // this image's one real /etc/passwd entry, for whatever XDG config
    // lookups cosmic-config's dependencies do internally.
    spawn_env(COSMIC_COMP, &[COSMIC_COMP], &[("XDG_RUNTIME_DIR", XDG_RUNTIME_DIR), ("HOME", "/root")])
}

/// devtmpfs already created device nodes before udevd started, but
/// without notifying it — udevd only learns about *new* uevents over its
/// netlink socket. `udevadm trigger` re-emits an "add" uevent for every
/// device already in sysfs so udevd's database actually reflects what's
/// there (a "coldplug"). One-shot, not supervised like the daemons above.
fn coldplug_devices() {
    let _ = std::process::Command::new(UDEVADM).arg("trigger").status();
}

/// Blocks (bounded) until udevd has finished processing the coldplug
/// queue above — cosmic-comp's DRM/libinput device probing needs the
/// /dev/dri and /dev/input nodes udevd creates to actually be there
/// first, unlike agetty/seatd/dbus, which don't touch devices at startup.
fn settle_devices() {
    let _ = std::process::Command::new(UDEVADM).args(["settle", "--timeout=10"]).status();
}

fn main() {
    mount_basic_filesystems();

    let mut udevd_pid = spawn_udevd();
    let mut seatd_pid = spawn_seatd();
    let mut dbus_pid = spawn_dbus();
    coldplug_devices();
    println!(
        "distro-init: starting {AGETTY} on tty1 and ttyS0, {SEATD}, {DBUS_DAEMON}, and {UDEVD}"
    );

    let mut tty1_pid = spawn_tty1();
    let mut serial_pid = spawn_serial();

    // Blocking here delays cosmic-comp's own start, not the login
    // prompts above (both gettys are already running by this point) —
    // the one-time cost of waiting for real device nodes buys not racing
    // cosmic-comp's DRM/libinput probing against udevd's coldplug queue.
    settle_devices();
    let mut cosmic_comp_pid = spawn_cosmic_comp();
    println!("distro-init: starting {COSMIC_COMP}");

    loop {
        match waitpid(None, Some(WaitPidFlag::empty())) {
            Ok(WaitStatus::Exited(pid, _)) | Ok(WaitStatus::Signaled(pid, _, _)) => {
                if pid == tty1_pid {
                    println!("distro-init: tty1 agetty exited, respawning");
                    tty1_pid = spawn_tty1();
                } else if pid == serial_pid {
                    println!("distro-init: ttyS0 agetty exited, respawning");
                    serial_pid = spawn_serial();
                } else if pid == seatd_pid {
                    println!("distro-init: seatd exited, respawning");
                    seatd_pid = spawn_seatd();
                } else if pid == dbus_pid {
                    println!("distro-init: dbus-daemon exited, respawning");
                    dbus_pid = spawn_dbus();
                } else if pid == udevd_pid {
                    println!("distro-init: udevd exited, respawning");
                    udevd_pid = spawn_udevd();
                } else if pid == cosmic_comp_pid {
                    println!("distro-init: cosmic-comp exited, respawning");
                    cosmic_comp_pid = spawn_cosmic_comp();
                }
                // Otherwise this was just reaping an orphaned child.
            }
            Ok(_) => {}
            Err(_) => {
                // No children currently waitable; avoid a busy-loop.
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}
