//! `distro`'s own PID 1: mounts the basic virtual filesystems and
//! supervises `agetty` on both the VGA console and the serial console,
//! respawning either if it exits and reaping any other orphaned children.
//! `agetty` execs `/bin/login` (shadow-utils) once a username is entered,
//! which authenticates against `/etc/passwd`/`/etc/shadow` and execs the
//! user's shell — this replaces Phase 1a's direct shell spawn.

use nix::mount::{mount, MsFlags};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::{execv, fork, ForkResult, Pid};
use std::ffi::CString;
use std::time::Duration;

const AGETTY: &str = "/sbin/agetty";

fn mount_basic_filesystems() {
    // Best-effort: devtmpfs is often already mounted by the kernel itself
    // (CONFIG_DEVTMPFS_MOUNT) before init even runs.
    let _ = mount(Some("proc"), "/proc", Some("proc"), MsFlags::empty(), None::<&str>);
    let _ = mount(Some("sysfs"), "/sys", Some("sysfs"), MsFlags::empty(), None::<&str>);
    let _ = mount(Some("devtmpfs"), "/dev", Some("devtmpfs"), MsFlags::empty(), None::<&str>);
}

fn spawn(path: &str, args: &[&str]) -> Pid {
    match unsafe { fork() } {
        Ok(ForkResult::Parent { child, .. }) => child,
        Ok(ForkResult::Child) => {
            let c_path = CString::new(path).unwrap();
            let c_args: Vec<CString> = args.iter().map(|a| CString::new(*a).unwrap()).collect();
            let _ = execv(&c_path, &c_args);
            // Only reached if exec itself failed (e.g. agetty missing).
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

fn main() {
    mount_basic_filesystems();
    println!("distro-init: starting {AGETTY} on tty1 and ttyS0");

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
