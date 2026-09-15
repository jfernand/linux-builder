#import "isss-template.typ": *

#show: isss-doc.with(
  title: "Two Linux Systems From Scratch",
  subtitle: "What a Linux System Is, How It Boots, and How distro/distroless Each Build One",
  author: "Javier Fernández",
  contact: "jfernand@me.com",
  date: "2026-09-15",
  docid: "ISSS-TR-0421",
  running: "Two Linux Systems From Scratch",
  abstract: [
    This workspace builds two independent, bootable Linux systems entirely
    from upstream source, using nothing but the host's own compiler
    toolchain: #cd[distroless] (musl + BusyBox + uutils, minimal) and
    #cd[distro] (glibc, from-scratch, aimed eventually at a COSMIC desktop).
    Part 1 explains what a Linux system fundamentally *is* — the
    bootloader/kernel/init layering every Linux system shares, and the
    handful of jobs (device management, seat/session, coreutils, graphics)
    every desktop-capable system needs done, one way or another. Part 2
    explains how this workspace's shared buildpack architecture works, and
    then walks through each distro's own concrete build: what packages it
    compiles, in what order, and how each has been verified to actually
    boot.
  ],
  meta: (
    ("Workspace", [Cargo workspace: #cd[buildpack-core] + #cd[buildpacks] (shared package registry) · #cd[builder-tui] (shared dashboard) · #cd[distroless] (musl/BusyBox) · #cd[distro] (glibc/from-scratch) · #cd[distro-init] (PID 1)]),
    ("Target", [Native #cd[x86_64-unknown-linux-gnu] — host toolchain, no cross-compilation]),
    ("Coverage", [Both distros' full pipelines, kernel through a booting, logged-in shell; #cd[distro] additionally through Weston actually rendering a client via virtio-gpu inside QEMU]),
    ("Not covered", [A Rust toolchain on #cd[distro]'s own target, COSMIC itself, real GPU drivers beyond virtio-gpu, audio, networking UI — see §11]),
  ),
)

#v(30pt)
#align(center)[
  #display(size: 46pt)[Part One]
  #v(2pt)
  #text(size: 12pt, fill: fg-secondary, weight: 500)[What a Linux system is, and how it boots]
]
#v(20pt)

= The Three Layers

Every Linux system, regardless of distribution, is the same three layers
stacked on top of each other:

#dtable(
  columns: (auto, 1fr),
  ([Layer], [Job]),
  ([Bootloader], [Runs first, finds a kernel image on disk, loads it into memory, hands off control. GRUB is the common choice; there are others.]),
  ([Kernel], [One binary — `vmlinuz`. Initializes hardware, mounts the root filesystem, then execs exactly one userspace program as process ID 1. Everything the kernel does before that point (driver probing, `devtmpfs`, memory management) is the same regardless of what distribution eventually boots.]),
  ([Userspace], [Everything else — starting with PID 1 (*init*) and everything init, directly or indirectly, starts after it.]),
)

What actually distinguishes one Linux system from another lives almost
entirely in that third layer: which init, which device manager, which
coreutils implementation, which graphics stack, if any. A minimal
embedded system and a full desktop share the exact same bootloader/kernel
handoff — they differ in how much userspace machinery init brings up
afterward.

= How It Boots, Step by Step

#dtable(
  columns: (auto, auto, 1fr),
  align: (left, left, left),
  ([Step], [Component], [What happens]),
  ([1], [Bootloader], [Reads the kernel image off disk and hands off.]),
  ([2], [Kernel], [Boots, mounts the root filesystem read-write, execs `/sbin/init`.]),
  ([3], [init (PID 1)], [Mounts whatever else the running system needs (`/proc`, `/sys`, `/dev`, `/run`), starts every other userspace service, and from then on supervises them — if a required service dies, PID 1 is what notices and (usually) restarts it.]),
  ([4], [Device management], [Started by init, early: makes the hardware devtmpfs already created nodes for actually usable — permissions, symlinks, a queryable "what's plugged in" database.]),
  ([5], [getty], [Opens a terminal (the console, a serial port, both) and prints a login prompt.]),
  ([6], [login], [Reads a username and password, checks them against the system's account database, and — if they're valid — replaces itself with the user's shell.]),
  ([7], [shell], [An interactive command interpreter. The system is now usable.]),
)

Steps 1–2 are identical on every Linux system. Steps 3 onward are where a
distribution's own choices show up — how much init does directly, what it
delegates to separate daemons, and what those daemons are.

== What init actually does, and why it's never "nothing"

PID 1 is not an implementation detail. The kernel will only ever execute
*one* program as PID 1, and that program inherits a genuinely special
job: it's the ancestor of every other userspace process, and it's what
the kernel hands orphaned child processes to for reaping. A system with
no working init doesn't boot to a shell — it boots to a kernel panic the
moment that one process exits or crashes.

Three different answers to "what should init actually do" show up across
real systems:

#dtable(
  columns: (auto, 1fr),
  ([Approach], [What it looks like]),
  ([systemd], [The default on most desktop distributions. Starts and supervises services in dependency order, described declaratively as "unit" files, alongside a large family of companion daemons (`logind` for sessions, `udevd` for devices, `journald` for logging, `networkd`, and more) that a full systemd-based system typically runs together.]),
  ([A small built-in init], [BusyBox ships one: reads a short, static `/etc/inittab`, starts and respawns exactly the processes it names. No dependency graph, no unit files — just a fixed list.]),
  ([A purpose-written init], [Nothing says init has to be either of the above. A minimal init can be a short, direct program: mount what's needed, fork a fixed set of children, wait for any of them to exit, fork it again. §9's `distro-init` is exactly this — about 100 lines.]),
)

None of these is "more correct" than the others — they're different
trade-offs between flexibility (systemd can express arbitrary service
dependency graphs) and simplicity (a fixed list of children is easy to
read in full and reason about completely).

= Device Management: udev

The kernel's device drivers know about hardware the moment it's detected,
but they don't manage `/dev` in any structured way themselves —
`devtmpfs`, mounted early in boot, creates basic device nodes
automatically as drivers load, and that's the extent of what it does on
its own. *udev* is the userspace daemon that actually does the rest: it
listens for the kernel's own hardware-change announcements (sent over a
netlink socket whenever something is plugged in, unplugged, or otherwise
changes state), and in response creates or removes the matching `/dev`
entries, applies permissions and naming rules, and maintains a live,
queryable database of what hardware currently exists.

Without something doing this job, nothing gets notified when a device
appears after boot, and nothing can ask "what's connected right now" in a
structured way — a real gap for anything beyond the most minimal system.
`eudev` is a fork of `udev` that works without systemd (ordinary `udev`
is a systemd subproject); it's one of the two device-management answers
this workspace actually uses (§10, §11).

= Seat & Session Management

A Wayland compositor needs exclusive, arbitrated access to raw input
devices (`/dev/input/*`) and the GPU (`/dev/dri/*`) — but running it as
root just to get that access is the wrong trade-off. *Seat management* is
the piece that solves this: a daemon holds those device files open on
behalf of whichever process currently "owns" the seat, and hands out
access over a socket instead. `systemd-logind` is the answer most
desktop distributions use; `seatd` is a smaller, systemd-independent
daemon that does the same one job and nothing else — its own
documentation describes it as depending only on libc.

= Userland Basics: coreutils, One Way or Another

Every Unix system needs a basic set of file and text commands — `ls`,
`cat`, `cp`, `mv`, `rm`, and so on. "Coreutils" is the generic name for
that toolset, not one specific program, and there are several real
answers to what actually provides it:

#dtable(
  columns: (auto, 1fr),
  ([Implementation], [What it is]),
  ([GNU coreutils], [The implementation most desktop Linux distributions ship — one binary per command.]),
  ([BusyBox], [A much older answer aimed at tiny/embedded systems: a single small C binary that crams coreutils *and* a shell, `init`, `mount`, `ps`, and dozens of other tools into one multi-call executable, symlinked under every command name it implements.]),
  ([uutils], [A from-scratch reimplementation of just coreutils, in Rust — one multi-call binary, the same symlink-per-applet shape as BusyBox, but scoped to coreutils alone rather than a whole system toolbox.]),
)

Whichever is chosen, the shell and login program are separate concerns —
coreutils gets you `ls` and `cp`, not a shell to type them into or a
login prompt to authenticate at.

= Graphics, Conceptually: DRM, Mesa, Gallium, DRI, EGL, GBM

Getting from "a kernel that can talk to a GPU" to "a compositor rendering
a client's window" passes through several distinct layers, each solving
a different part of the problem:

#dtable(
  columns: (auto, 1fr),
  ([Layer], [What it actually is]),
  ([DRM (kernel)], [The kernel subsystem that owns the GPU at the lowest level: hands out framebuffers, submits command buffers. Has no idea what "draw a triangle" means — purely a resource-management and submission interface.]),
  ([Mesa], [The userspace library that turns OpenGL/OpenGL ES calls into whatever a specific GPU actually understands.]),
  ([Gallium], [Mesa's own internal plumbing for doing that translation once per GPU *family* rather than once per API. A "Gallium driver" is the translator for one specific GPU — or, for a virtual machine, one specific *virtual* GPU.]),
  ([DRI], [Direct Rendering Infrastructure — the convention by which an application actually finds and loads the right driver at runtime.]),
  ([EGL], [The glue between a window system (Wayland, X11, …) and an OpenGL/GLES context. What a compositor or client actually links against directly — not Mesa's internals.]),
  ([GBM], [Generic Buffer Management — how a Wayland compositor allocates the actual pixel buffers it hands to the DRM/KMS display hardware. The piece that makes a rendered frame show up on screen rather than just existing in GPU memory.]),
)

A minimal Wayland-only graphics stack built from source needs, at
minimum: the kernel's DRM driver for the target GPU, `libdrm` (the
kernel-userspace ioctl wrapper every one of these libraries builds on),
and Mesa itself providing `libEGL`/`libGLESv2`/`libgbm` plus at least one
Gallium driver. `llvmpipe` (an LLVM-based software rasterizer) is one
possible fallback driver, not a hard requirement — `virgl` (talks to a
virtual machine's virtio-gpu/virgl backend) and `softpipe` (a
non-LLVM software fallback) are two others that work without pulling
LLVM into the build at all.

= Static vs Dynamic Linking

A binary can be built two ways: *statically linked*, where every library
it needs is compiled directly into the one executable file, or
*dynamically linked*, where the executable instead records which shared
libraries (`.so` files) it needs and expects to find them on the target
system at run time. Static linking means "compile once, copy one file,
done" — no shared-library bookkeeping on the target at all — but it only
scales to programs whose dependencies are genuinely small and
self-contained. Anything that needs a library with real runtime state or
plugin-style extensibility (D-Bus, a graphics driver stack) is
effectively required to link dynamically, which means the target system
needs those shared libraries physically present and findable by the
dynamic linker before the program will run at all.

#pagebreak()
#v(20pt)
#align(center)[
  #display(size: 46pt)[Part Two]
  #v(2pt)
  #text(size: 12pt, fill: fg-secondary, weight: 500)[How distroless and distro each build one]
]
#v(20pt)

= This Workspace

#dtable(
  columns: (auto, 1fr),
  ([Crate], [Role]),
  ([#cd[buildpack-core]], [Foundational library: the `Buildpack` trait every package implements, `DistroConfig` (the shared config-file format), the `PipelineStage` trait for whole-image operations, and the dependency-graph/build helpers both distros use.]),
  ([#cd[buildpacks]], [One implementation per upstream package — kernel, coreutils, every library in §10–11's tables — shared between whichever distro(s) actually use each one.]),
  ([#cd[builder-tui]], [A generic interactive terminal dashboard for running any stage of either distro's pipeline and writing the result to a USB stick.]),
  ([#cd[distroless]], [musl + BusyBox + uutils — a small, minimal distro. §10.]),
  ([#cd[distro]], [A from-scratch *glibc* system, native-compiled, aimed eventually at a COSMIC desktop. §11.]),
  ([#cd[distro-init]], [A from-scratch PID 1 written for this project, \~100 lines of Rust — used by `distro` (§11.1).]),
)

== The shared buildpack architecture

Every upstream package this workspace builds — for either distro — is
one self-contained value implementing the `Buildpack` trait: it declares
its own source location, its dependencies on other buildpacks, how to
fetch and build it, and what files it produces.

#dtable(
  columns: (auto, 1fr),
  ([Concept], [What it does]),
  ([`Buildpack` trait], [`fetch()`/`build()`/`outputs()`/`clean()` plus a `describe()` for the TUI. One implementation per upstream package, in `buildpacks`.]),
  ([`DistroConfig`], [The shared config-file format both `distro.toml` and `distroless.toml` load through — `build_dir`, `networking`, and `image` are the only fields it understands directly; every other section is handed, unparsed, straight to that package's own buildpack.]),
  ([Dependency graph], [A real topological sort over every buildpack's declared dependencies decides build order — not a hand-maintained call sequence. Rendered fresh to an SVG (`dependency-graph.svg`) on every build.]),
  ([`PipelineStage` trait], [For the three operations that act on the whole assembled image rather than any one package: partitioning and writing the disk image, booting it in QEMU, and writing it to a USB stick. Identical logic for both distros.]),
  ([CLI subcommands], [`list-packages`, `fetch-pkg <id>`, `build-pkg <id>`, `clean-pkg <id>` — inspect or rebuild one package at a time, on either distro.]),
  ([`builder-tui`], [An interactive dashboard driven by the same registry of buildpacks — shows build status per package, runs any stage, walks through settings (networking, kernel features, hostname), and writes to a USB stick with a confirmation prompt.]),
)

Each package's config lives under a `[section]` named after its buildpack
id — a version and a source URL, at minimum. `distro.toml` and
`distroless.toml` differ only in which sections they define, not in
format.

= How distroless Builds a Linux System

`distroless` is deliberately the smallest of the two: musl libc,
BusyBox for the entire coreutils/shell/init/device-management job it can
cover, plus uutils and the kernel as the two things built from source
beyond BusyBox itself. Everything in it is cross-compiled against musl
rather than the host's own glibc.

== Boot sequence

#dtable(
  columns: (auto, auto, 1fr),
  align: (left, left, left),
  ([Step], [Component], [What happens]),
  ([1–2], [GRUB, kernel], [Same as §2's generic sequence.]),
  ([3], [BusyBox init], [Reads `/etc/inittab`: runs `/etc/init.d/rcS` once at boot, then starts and respawns `getty` on the console.]),
  ([4], [`rcS`], [A short shell script: mounts `/proc`/`/sys`/`devtmpfs`, sets the hostname, and — if networking is enabled — brings up `lo` and runs `udhcpc` for a DHCP lease.]),
  ([5–7], [getty → login → shell], [BusyBox's own `getty`/`login`/`ash` — a single root account with no password set.]),
)

== Packages

#dtable(
  columns: (auto, auto, 1fr),
  align: (left, left, left),
  ([Package], [Provides], [Notes]),
  ([Linux kernel], [`vmlinuz`], [The same `Kernel` buildpack `distro` uses — kernel builds have no musl/glibc-specific behavior.]),
  ([uutils/coreutils], [`ls`, `cat`, `cp`, …], [The musl variant of the same `Uutils` buildpack `distro` uses (§11.4) — one multi-call binary, statically linked, musl-static by default.]),
  ([BusyBox], [shell, init, `mount`, `getty`, and the rest of a minimal system toolbox], [Built via a curated Kconfig: `allnoconfig`, then a specific applet list enabled and cross-compiled against musl, statically linked.]),
)

== Pipeline

#codepanel(title: "distroless's CLI surface")[
```
distroless fetch                    # download + extract kernel, uutils, busybox
distroless build-toolchain          # install musl-gcc + the musl Rust target (once)
distroless build-kernel             # the kernel buildpack
distroless build-userland           # uutils + busybox
distroless assemble-rootfs          # merge it all into build/rootfs
distroless make-image               # partition + GRUB + write the disk image
distroless test-qemu [--window]     # boot it
distroless write-usb --device <dev> # dd to real hardware (destructive)
distroless tui                      # interactive dashboard for all of the above
distroless all                      # the whole pipeline, in order
```
]

Assembling the rootfs copies uutils' and BusyBox's declared outputs in
directly (uutils' applet symlinks, BusyBox's own `bin/sh`, `sbin/init`,
and applet set), installs the kernel modules this build produced, and
writes the handful of config files BusyBox's init expects
(`/etc/inittab`, `/etc/passwd`, `/etc/fstab`, `/etc/init.d/rcS`, and —
when networking is enabled — a udhcpc bound/renew/deconfig script).

#callout(kind: "ok", "Verified")[
  A scripted QEMU boot reaches the login prompt, logs in via BusyBox
  `ash` with no password, and confirms `ls /bin | wc -l` and a correct
  `uname -a` against the just-built kernel.
]

= How distro Builds a Linux System

`distro` is the larger of the two: a from-scratch *glibc* system,
native-compiled against the host's own toolchain — no cross-compilation
— aimed eventually at running a COSMIC desktop. Where `distroless`
reaches for one BusyBox binary to cover coreutils, shell, init, and
device management all at once, `distro` builds a separate,
purpose-specific package for each of those jobs, plus (currently) the
full graphics stack needed to host a Wayland compositor.

== Boot sequence

#dtable(
  columns: (auto, auto, 1fr),
  align: (left, left, left),
  ([Step], [Component], [What happens]),
  ([1–2], [GRUB, kernel], [Same as §2's generic sequence.]),
  ([3], [distro-init], [Our own PID 1 (below). Mounts `/proc`, `/sys`, `/dev` (devtmpfs), and a fresh `tmpfs` at `/run`.]),
  ([4], [udevd, seatd, dbus-daemon], [Forked and exec'd, in that order. `distro-init` then runs `udevadm trigger` once — a *coldplug* that tells `udevd` about the devices devtmpfs already created before it started.]),
  ([5], [agetty ×2], [Forked on `tty1` (VGA console) and `ttyS0` (serial, for QEMU/headless use). Whichever exits gets respawned.]),
  ([6], [login], [`agetty` execs `/bin/login` once a username is typed. Reads `/etc/passwd`/`/etc/shadow` for real — the `root` account's shadow entry has an empty password field, so `login` skips the password prompt.]),
  ([7], [bash], [`login` execs the shell named in `/etc/passwd`.]),
)

#callout(kind: "ok", "Verified live")[
  Every arrow above was reproduced in QEMU with a scripted serial-console
  session: `login: root` → no password prompt → `-bash-5.2#` →
  `dbus-send --system … ListNames` returns a real reply → `udevadm info
  --query=all --name=/dev/tty1` returns a populated device entry.
]

=== distro-init

Nothing glibc-based has a BusyBox-equivalent single init binary, and
pulling in systemd would drag in `logind`, `udevd`, `journald`, and far
more than this system currently needs. `distro-init` is a purpose-built
init instead — about 100 lines of Rust using the `nix` crate — following
exactly the "purpose-written init" pattern described in §3.1.

#codepanel(title: "distro-init/src/main.rs — the whole supervision loop, abbreviated")[
```rust
fn main() {
    mount_basic_filesystems();          // proc, sysfs, devtmpfs, tmpfs at /run

    let mut udevd_pid = spawn_udevd();  // no -d: stays a tracked child, not a daemon
    let mut seatd_pid = spawn_seatd();
    let mut dbus_pid  = spawn_dbus();   // --nofork, same reasoning
    coldplug_devices();                 // `udevadm trigger`, one-shot

    let mut tty1_pid   = spawn_tty1();  // agetty
    let mut serial_pid = spawn_serial();

    loop {
        match waitpid(None, Some(WaitPidFlag::empty())) {
            Ok(WaitStatus::Exited(pid, _)) | Ok(WaitStatus::Signaled(pid, _, _)) => {
                // whichever of the five died gets forked again; anything
                // else reaped here was just an orphaned grandchild.
            }
            ...
        }
    }
}
```
]

Every daemon it starts would normally daemonize itself (fork, detach,
exit the parent) — `dbus-daemon --nofork` and `udevd` with no `-d`
suppress that, so they stay `distro-init`'s direct children and its
`waitpid` loop actually sees them exit if they crash.

== The kernel

`distro.toml` names a kernel version and a download URL under `[kernel]`;
`distro build-kernel` fetches, configures, and compiles it. The starting
config is either `make defconfig` (the default) with every optional
*feature pack* below stripped back off, or a previously saved
`.config` from an interactive `make menuconfig` session
(`distro menu-config`), re-resolved via `make olddefconfig`. Either way,
whatever packs are named in `kernel.features` get turned back on as the
final step.

#dtable(
  columns: (auto, 1fr),
  align: (left, left),
  ([Pack], [What it turns on]),
  ([`graphics`], [DRM/KMS graphics + fbdev console (i915, virtio-gpu, bochs, AGP) instead of plain VGA text.]),
  ([`sound`], [ALSA sound subsystem and the Intel HDA driver.]),
  ([`wireless`], [The Wi-Fi stack (cfg80211/mac80211) and rfkill.]),
  ([`hid-extras`], [Per-vendor HID quirk drivers and the hiddev/hidraw userspace interfaces — generic USB HID keyboards/mice work without this.]),
  ([`legacy-nics`], [Dedicated Ethernet chipset drivers for real hardware — QEMU's virtio-net always works without this.]),
  ([`legacy-buses`], [PCMCIA/CardBus and legacy PATA chipset drivers.]),
  ([`network-fs`], [NFS (client + root-over-NFS), 9P, and autofs.]),
  ([`netfilter`], [Connection tracking, NAT, and iptables.]),
  ([`security-extras`], [Disk quotas, POSIX ACLs, and SELinux.]),
  ([`iommu`], [AMD/Intel IOMMU support — PCI passthrough or virtualization-host use.]),
  ([`debug`], [Kernel debug instrumentation — useful while bringing up boot, dead weight once stable.]),
  ([`ia32-emulation`], [Run 32-bit x86 binaries on this 64-bit kernel.]),
  ([`iso9660`], [ISO9660/Joliet/zisofs filesystem support.]),
  ([`boot-logo`], [Framebuffer console + boot-time logo (stock penguin, or a custom 80×80 PPM via `kernel.logo_file`).]),
)

`distro.toml` currently sets no `kernel.features` at all — the kernel
ships the fully stripped `defconfig` baseline plus whatever `graphics`
support the rootfs actually needs is enabled directly, not via a named
pack.

== Packages

Five packages exist purely to get from a mounted rootfs to a real,
authenticated shell — the glibc equivalent of what BusyBox does as one
binary, spread across separate upstream projects because glibc-land has
no single equivalent to reach for. All five are statically linked: one
binary compiled, one binary copied into the rootfs, nothing else to
track.

#dtable(
  columns: (auto, auto, auto, 1fr),
  align: (left, left, left, left),
  ([Package], [Version], [Provides], [Why it's there]),
  ([uutils/coreutils], [git `main`], [`ls`, `cat`, `cp`, …], [Rust reimplementation of GNU coreutils, chosen over BusyBox for this distro specifically to prefer Rust alternatives where practical.]),
  ([bash], [5.2.37], [`/bin/bash`, `/bin/sh`], [The login shell named in `/etc/passwd`.]),
  ([util-linux], [2.41.2], [`agetty`, `mount`, `umount`], [Built with only these three programs enabled — util-linux ships dozens, only these are needed here.]),
  ([shadow-utils], [4.17.4], [`login`, `passwd`], [Real `/etc/passwd` + `/etc/shadow` authentication.]),
)

Three daemons do what a desktop session needs before any compositor can
run — none of them optional, the floor a compositor stands on rather
than decoration:

#dtable(
  columns: (auto, auto, 1fr),
  align: (left, left, left),
  ([Package], [Version], [What it actually does]),
  ([seatd], [0.9.3], [Owns `/dev/input/*` and `/dev/dri/*` on behalf of whatever process asks over its socket — the seat-management daemon described conceptually in §4, chosen over `systemd-logind` to avoid pulling in the rest of systemd.]),
  ([dbus], [1.16.2], [The system message bus. Runs as `root` here — the rootfs has no unprivileged `messagebus` user yet to drop privileges to.]),
  ([eudev], [3.2.14], [The systemd-independent `udev` fork described in §3. Exists in this pipeline specifically because `libinput` hard-depends on `libudev`.]),
)

Seven libraries sit ready for a compositor to link against — nothing in
this group runs on its own; verification at this layer is "builds and
installs cleanly," not "does something observable":

#dtable(
  columns: (auto, auto, 1fr),
  align: (left, left, left),
  ([Package], [Version], [What it's for]),
  ([wayland], [1.26.0], [The core wire-protocol libraries and `wayland-scanner`, the code generator every later Wayland package needs at its own build time.]),
  ([wayland-protocols], [1.49], [The actual protocol definitions — `xdg-shell` and the rest — as XML.]),
  ([libxkbcommon], [1.12.4], [Turns "us, evdev, pc105" into the keymap tables a compositor hands to clients.]),
  ([pixman], [0.46.4], [Software rasterization — Mesa's fallback path.]),
  ([libdisplay-info], [0.4.0], [Parses a monitor's own EDID/DisplayID.]),
  ([libevdev], [1.13.7], [Reads and writes raw evdev input-device events — `libinput`'s one mandatory dependency.]),
  ([libinput], [1.31.3], [Turns raw evdev events into pointer/keyboard/touch/gesture events a compositor actually wants.]),
)

The graphics stack proper — from §6's conceptual layering down to actual
built libraries, scoped to QEMU's own virtual GPU rather than real
hardware:

#dtable(
  columns: (auto, auto, 1fr),
  align: (left, left, left),
  ([Package], [Version], [What it's for]),
  ([libdrm], [2.4.134], [The kernel-userspace ioctl wrapper every GPU-facing library builds on — every vendor-specific sub-library disabled, virtio-gpu needs only the generic core.]),
  ([Mesa], [26.2.2], [`libEGL`, `libGLESv2`, `libgbm`, and the Gallium driver — built scoped to `virgl` (talks to QEMU's virtio-gpu/virgl backend) and `softpipe` (software fallback). Vulkan and GLX/X11 both disabled — Wayland/EGL/GLES only, no LLVM needed for either driver.]),
)

Hosting an actual compositor needs six more packages, plus a keyboard
layout data package:

#dtable(
  columns: (auto, auto, 1fr),
  align: (left, left, left),
  ([Package], [Version], [What it's for]),
  ([zlib], [1.3.2], [Compression — libpng's and cairo's own dependency.]),
  ([expat], [2.7.1], [XML parsing — fontconfig's config-file dependency.]),
  ([libpng], [1.6.44], [PNG images — Weston's own unconditional dependency, and cairo's PNG surface backend.]),
  ([FreeType], [2.13.3], [Font rasterization.]),
  ([fontconfig], [2.15.0], [Font matching — FreeType's companion, needed for cairo's font backend.]),
  ([cairo], [1.18.2], [2D graphics — Weston's mandatory window-decoration dependency. Image-surface only, no X11/xcb/GL backends.]),
  ([xkeyboard-config], [2.44], [Keyboard layout/rules *data* — no code, what `libxkbcommon` looks for at runtime.]),
  ([Weston], [16.0.0], [The compositor itself — DRM backend, GL renderer, kiosk shell, `weston-simple-egl`. Vulkan/X11/Xwayland/systemd/JPEG/WebP/LCMS2 all disabled.]),
)

#callout(kind: "ok", "Verified — the actual milestone")[
  A real QEMU boot: Weston (launched with a minimal `weston.ini`
  selecting its kiosk shell) initializes DRM, EGL/GL via this workspace's
  own Mesa `softpipe` Gallium driver, `libinput`, and the kiosk shell,
  and opens a Wayland socket. `weston-simple-egl`, run against that
  socket, connects and renders without error — a minimal Wayland client
  actually rendering via virtio-gpu inside QEMU. Not yet automatic at
  boot: today Weston is started by hand from a login shell, not a
  `distro-init` service.
]

== The sysroot: how these packages find each other

Static-base packages (§11.4's first table) never need each other at
build time — each just needs the host's gcc. Everything from the seat
layer onward does: `wayland-protocols` needs `wayland-scanner` on `PATH`
at its own build time, and `libinput` needs `eudev`'s installed
`libudev.pc` to link against `libudev` at all. Every one of those
packages is therefore built with `--prefix=/usr` (its normal, final,
"as if genuinely installed" prefix) and installed with
`DESTDIR=<sysroot>` — files physically land under a shared sysroot
directory, but each package's own compiled-in idea of its prefix stays
`/usr`, which matters: some of these daemons look up their own config
files relative to whatever prefix they were *actually built with*, at
their own runtime, on the real target. `PKG_CONFIG_SYSROOT_DIR`
(pkg-config's own mechanism for exactly this case) rewrites the
`-I`/`-L` paths a later package's build sees from `/usr/...` to the
sysroot's real, on-disk `<sysroot>/usr/...`. Assembling the rootfs then
copies that whole sysroot tree in with one `cp -a`.

== Pipeline

#codepanel(title: "distro's CLI surface")[
```
distro fetch                    # download + extract every source
distro build-toolchain          # apt-get the host build tools (once)
distro build-kernel             # the kernel buildpack
distro build-userland           # every other package
distro assemble-rootfs          # merge it all into build-distro/rootfs
distro make-image               # partition + GRUB + write the disk image
distro test-qemu [--window]     # boot it
distro write-usb --device <dev> # dd to real hardware (destructive)
distro tui                      # interactive dashboard for all of the above
distro all                      # the whole pipeline, in order
```
]

`build-userland` fetches and builds every non-kernel package in
dependency order (§9.1's topological sort); `assemble-rootfs` then
copies every statically-linked package's declared outputs in directly
(coreutils and its applet symlinks, bash + `bin/sh`, util-linux's
`agetty`/`mount`/`umount`, shadow's `login`/`passwd`, `distro-init` as
`/sbin/init`), bulk-copies the whole sysroot for every dynamically-linked
package in one `cp -a`, then adds the host's own C library, libstdc++,
and the handful of other runtime `.so`s these dynamically-linked
packages need, plus the dynamic linker itself and `/etc/passwd` +
`/etc/shadow`.

#callout(kind: "ok", "Verified")[
  A full pipeline run (`fetch` → `build-kernel` → `build-userland` →
  `assemble-rootfs` → `make-image`) reports every package "already
  exists" on a rebuild, and a full QEMU regression boot — login,
  `dbus-send`, `udevadm`, and Weston's own DRM/EGL/GL initialization —
  passes end to end.
]

= What Isn't Part of the Picture Yet

#spec(
  ("Next", [`rustup`/`cargo` on-target, plus a curated Rust-CLI-tools suite (ripgrep, bat, eza, …) — no new architecture needed, each is just another buildpack.]),
  ("Then", [COSMIC itself — `cosmic-comp`, `cosmic-session`, `cosmic-panel`, `cosmic-greeter`, minimal subset first.]),
  ("Later", [More COSMIC components, real GPU drivers beyond `virtio-gpu`, audio, networking UI.]),
)

And three deliberate gaps in what's already built, worth knowing about
rather than discovering later:

- uutils' built feature set does not include `grep` or `sed` — they were
  never part of coreutils' scope upstream in the first place. The applet
  symlinks exist but dangle. Not yet fixed.
- `libxcb` was never built. Its only real consumer on a Wayland-native
  target is XWayland compatibility, not currently planned.
- `mtdev` (legacy multitouch) and `libwacom` (tablet identification) were
  both left out of `libinput` — niche hardware support, easy to add back
  later if a real device needs it.
