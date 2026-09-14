//! `distro`'s own PID 1: mounts the basic virtual filesystems, starts
//! seatd and dbus (Phase 2's seat/session plumbing) and supervises
//! `agetty` on both the VGA console and the serial console, respawning
//! any of the three if it exits and reaping any other orphaned children.
//! `agetty` execs `/bin/login` (shadow-utils) once a username is entered,
//! which authenticates against `/etc/passwd`/`/etc/shadow` and execs the
//! user's shell — this replaces Phase 1a's direct shell spawn.

use nix::mount::{mount, MsFlags};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::{execv, fork, ForkResult, Pid};
use std::ffi::CString;
use std::time::Duration;

const AGETTY: &str = "/sbin/agetty";
const SEATD: &str = "/usr/bin/seatd";
const DBUS_DAEMON: &str = "/usr/bin/dbus-daemon";

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
}

fn spawn(path: &str, args: &[&str]) -> Pid {
    match unsafe { fork() } {
        Ok(ForkResult::Parent { child, .. }) => child,
        Ok(ForkResult::Child) => {
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

fn main() {
    mount_basic_filesystems();

    let mut seatd_pid = spawn_seatd();
    let mut dbus_pid = spawn_dbus();
    println!("distro-init: starting {AGETTY} on tty1 and ttyS0, {SEATD}, and {DBUS_DAEMON}");

    let mut tty1_pid = spawn_tty1();
    let mut serial_pid = spawn_serial();

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
