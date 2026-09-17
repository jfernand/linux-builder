//! `distro`'s own PID 1: mounts the basic virtual filesystems, starts
//! udevd/seatd/dbus (Phase 2's device-management and seat/session
//! plumbing), `cosmic-comp` and `cosmic-bg` (COSMIC's compositor and
//! background renderer, §10.3.6 of the phase report) once devices have
//! settled — but only if a `distro.toml` build actually installed them;
//! the cosmic-kiosk bundle is an opt-out-able "final" pack pair (§11), so
//! this checks for the binaries rather than assuming they exist — and
//! supervises `agetty` on both the VGA console and the serial console.
//! Every present service gets respawned if it exits, and any other
//! orphaned child gets reaped. `agetty` execs `/bin/login` (shadow-utils)
//! once a username is entered, which authenticates against
//! `/etc/passwd`/`/etc/shadow` and execs the user's shell — this replaces
//! Phase 1a's direct shell spawn.

use nix::mount::{mount, MsFlags};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::{execv, fork, ForkResult, Pid};
use std::ffi::CString;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;

const AGETTY: &str = "/sbin/agetty";
const SEATD: &str = "/usr/bin/seatd";
const DBUS_DAEMON: &str = "/usr/bin/dbus-daemon";
const UDEVD: &str = "/usr/sbin/udevd";
const UDEVADM: &str = "/usr/bin/udevadm";
const COSMIC_COMP: &str = "/usr/bin/cosmic-comp";
const COSMIC_BG: &str = "/usr/bin/cosmic-bg";
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
    // wlroots' shm-based wl_shm/dmabuf-feedback format-table allocation
    // (shm_open, not memfd_create) needs a real /dev/shm — devtmpfs above
    // doesn't provide one, so without this wlroots clients (sway) fail
    // immediately with "Failed to allocate shm file for format table".
    // cosmic-comp/smithay never hit this gap since it prefers memfd_create.
    // devtmpfs just freshly overmounted /dev above, so /dev/shm as a mount
    // point doesn't exist yet either — create it first.
    let _ = std::fs::create_dir("/dev/shm");
    let _ = mount(Some("tmpfs"), "/dev/shm", Some("tmpfs"), MsFlags::empty(), None::<&str>);
    // Any terminal emulator that opens /dev/ptmx to allocate a PTY for its
    // shell (Alacritty, cosmic-term, foot) needs the resulting slave device
    // to resolve under /dev/pts/N — without devpts mounted here, /dev/ptmx
    // itself exists (a devtmpfs-provided device node) but grantpt()/
    // ptsname() on it fails, since there's no multi-instance devpts
    // filesystem backing it. No `gid=` override: this rootfs's /etc/group
    // (shadow's own minimal output) has no `tty` group to reference, so
    // PTY slaves stay root:root like every other device node here.
    let _ = std::fs::create_dir("/dev/pts");
    let _ = mount(Some("devpts"), "/dev/pts", Some("devpts"), MsFlags::empty(), Some("mode=0620"));
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
    // --autologin root: cosmic-comp/cosmic-bg take DRM/KMS ownership away
    // from tty1's own text console within a few seconds of boot (§10.3.6.1),
    // which in practice means there's no reliable window of time to see a
    // login prompt here at all, let alone type into it, before it's gone —
    // the root cause of the tty1 auto-start in `/root/.bash_profile` never
    // actually firing in practice. Autologin removes the human-typing step
    // that race depended on; root's password is already empty (see
    // `write_login_config`), so this doesn't weaken anything real.
    spawn(AGETTY, &[AGETTY, "--autologin", "root", "38400", "tty1"])
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

fn spawn_cosmic_bg() -> Pid {
    // cosmic-comp never names its socket "wayland-0": smithay's
    // ListeningSocketSource::new_auto() deliberately starts at 1 ("we
    // don't try wayland-0 since clients may connect to the wrong
    // compositor"), so a client that leaves WAYLAND_DISPLAY unset (which
    // defaults to "wayland-0") never finds it. Hardcoding "wayland-1" is
    // safe here since cosmic-comp is the only Wayland server this image
    // ever runs — nothing else could take that name first.
    //
    // cosmic-comp's own Wayland socket may also not exist yet the first
    // time this runs — cosmic-bg just fails fast (connect_to_env has no
    // retry), and the respawn-on-exit loop below tries again, the same
    // self-healing pattern every other service here already relies on
    // rather than a hand-tuned startup delay.
    spawn_env(
        COSMIC_BG,
        &[COSMIC_BG],
        &[("XDG_RUNTIME_DIR", XDG_RUNTIME_DIR), ("HOME", "/root"), ("WAYLAND_DISPLAY", "wayland-1")],
    )
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
    // cosmic-comp/cosmic-bg are the one pair of services here that's
    // meant to be genuinely optional (§11's "final packages, opt-out/in"
    // — a distro.toml build without the cosmic-kiosk bundle just never
    // installs these binaries at all), so — unlike every other spawn_*
    // above, which assumes its binary exists because build-time made it
    // required — this checks first and skips silently if absent, rather
    // than crash-looping forever on a missing executable.
    let mut cosmic_comp_pid = Path::new(COSMIC_COMP).exists().then(spawn_cosmic_comp);
    let mut cosmic_bg_pid = Path::new(COSMIC_BG).exists().then(spawn_cosmic_bg);
    match (cosmic_comp_pid.is_some(), cosmic_bg_pid.is_some()) {
        (true, true) => println!("distro-init: starting {COSMIC_COMP} and {COSMIC_BG}"),
        (true, false) => println!("distro-init: starting {COSMIC_COMP} ({COSMIC_BG} not installed)"),
        (false, _) => println!("distro-init: {COSMIC_COMP} not installed, skipping the desktop"),
    }

    loop {
        match waitpid(None, Some(WaitPidFlag::empty())) {
            Ok(status @ (WaitStatus::Exited(pid, _) | WaitStatus::Signaled(pid, _, _))) => {
                // Distinguishing a clean exit(code) from a raw signal
                // (SIGSEGV, SIGABRT, ...) matters here: a Rust panic
                // normally prints its own message before exiting 101, so
                // silence plus this line naming a signal instead is the
                // tell that something crashed below the language runtime.
                let reason = match status {
                    WaitStatus::Exited(_, code) => format!("exit code {code}"),
                    WaitStatus::Signaled(_, signal, _) => format!("signal {signal:?}"),
                    _ => unreachable!(),
                };
                if pid == tty1_pid {
                    println!("distro-init: tty1 agetty exited ({reason}), respawning");
                    tty1_pid = spawn_tty1();
                } else if pid == serial_pid {
                    println!("distro-init: ttyS0 agetty exited ({reason}), respawning");
                    serial_pid = spawn_serial();
                } else if pid == seatd_pid {
                    println!("distro-init: seatd exited ({reason}), respawning");
                    seatd_pid = spawn_seatd();
                } else if pid == dbus_pid {
                    println!("distro-init: dbus-daemon exited ({reason}), respawning");
                    dbus_pid = spawn_dbus();
                } else if pid == udevd_pid {
                    println!("distro-init: udevd exited ({reason}), respawning");
                    udevd_pid = spawn_udevd();
                } else if cosmic_comp_pid == Some(pid) {
                    println!("distro-init: cosmic-comp exited ({reason}), respawning");
                    cosmic_comp_pid = Some(spawn_cosmic_comp());
                } else if cosmic_bg_pid == Some(pid) {
                    println!("distro-init: cosmic-bg exited ({reason}), respawning");
                    cosmic_bg_pid = Some(spawn_cosmic_bg());
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
