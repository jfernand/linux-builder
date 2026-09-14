#import "isss-template.typ": *

#show: isss-doc.with(
  title: "Anatomy of Distro",
  subtitle: "How a From-Scratch glibc Linux Is Put Together, Piece by Piece",
  author: "Javier Fernández",
  contact: "jfernand@me.com",
  date: "2026-09-14",
  docid: "ISSS-TR-0421",
  running: "Anatomy of Distro · a piece-by-piece build guide",
  abstract: [
    #cd[distro] is a Rust CLI that builds a bootable, glibc-based Linux
    system entirely from upstream source, using nothing but the host's own
    compiler toolchain. This report explains how the thing is actually put
    together: what runs first when the machine powers on, what each binary
    and library in the image is for, how they depend on one another, and how
    the build pipeline turns seventy-odd separate upstream projects into one
    disk image. It is organized the way the system itself is layered — kernel,
    init, a static POSIX base, the seat/session daemons, the Wayland-core
    libraries, and the graphics stack, up through Weston itself actually
    rendering a client via virtio-gpu inside QEMU — rather than as a
    chronological log of work done.
  ],
  meta: (
    ("Workspace", [Cargo workspace: #cd[builder-core] (lib) · #cd[distroless] (musl/BusyBox) · #cd[distro] (glibc/from-scratch) · #cd[distro-init] (PID 1)]),
    ("Target", [Native #cd[x86_64-unknown-linux-gnu] — host toolchain, no cross-compilation]),
    ("Coverage", [Everything built and QEMU-verified through Phase 2, plus all of Phase 3: kernel graphics support, libdrm, Mesa, Weston and its cairo/xkeyboard-config chain, actually running with a client connected and rendering via virtio-gpu inside QEMU — see §7. Also: a new #cd[Buildpack] trait — all 11 proven packages, including kernel/util-linux/Mesa cut over from the old pipeline, are now what `distro build-userland`/`build-kernel` actually call, one command for all 25 packages — see §8.5]),
    ("Not covered", [Phase 4 through Phase 6 — a Rust toolchain on-target, COSMIC itself, real GPU drivers beyond virtio-gpu, audio, networking UI — see §9]),
  ),
)

= What Distro Is

Most ways to get a custom Linux system booting quickly start from someone
else's finished distribution — `debootstrap`, a container base image, an
Arch `pacstrap` — and layer changes on top. This project rejected that:
every binary in the built image is compiled here, from source, against
nothing but the host machine's own glibc and gcc. The one exception is the
same one every real distribution makes: the host's *compiler toolchain* —
gcc, binutils, meson, autoconf — is infrastructure, not distro content, the
same way a bootstrap compiler is infrastructure for a self-hosting language.

#dtable(
  columns: (auto, 1fr),
  ([Crate], [Role]),
  ([#cd[builder-core]], [Shared library: kernel build (`FEATURE_PACKS`), disk-image assembly, the QEMU test harness, the USB writer. Used by both distros unchanged.]),
  ([#cd[distroless]], [A separate, minimal distro built by this same workspace: musl + BusyBox + uutils, cross-compiled. Not covered here.]),
  ([#cd[distro]], [The subject of this report: a from-scratch *glibc* system, native-compiled, aimed eventually at a COSMIC desktop.]),
  ([#cd[distro-init]], [A from-scratch PID 1 written for this project, \~100 lines of Rust — see §2.2.]),
)

#spec(
  ("§ 2", [The boot sequence — what actually happens, in order, from power-on to a shell.]),
  ("§ 3", [The kernel — the config baseline, the feature packs, and how to pick a version.]),
  ("§ 4", [The static base: coreutils, shell, login — one binary in, no shared-library bookkeeping.]),
  ("§ 5", [The seat/session layer: seatd, dbus, eudev — what each one actually does.]),
  ("§ 6", [The Wayland-core libraries — built, installed, not yet used by anything.]),
  ("§ 7", [The graphics stack: libdrm, Mesa, what DRI/Gallium/EGL/GBM actually are, and Weston's own cairo dependency chain.]),
  ("§ 8", [How it's all actually built: the pipeline, the config file, the sysroot, and a new buildpack architecture in progress.]),
  ("§ 9", [What isn't part of the picture yet.]),
)

= The Boot Sequence

The clearest way to see how the pieces fit is to follow what actually
happens, in order, when the built image boots. At a high level, every Linux
system does the same three things: a *bootloader* (GRUB) finds and loads the
*kernel*; the kernel initializes hardware, mounts the root filesystem, and
executes exactly one program as process ID 1 — conventionally called
*init*; and everything else on the system is, directly or indirectly,
started by that init process. What differs system to system is entirely
what init actually does next — systemd starts hundreds of units in
dependency order; BusyBox's init reads a small `/etc/inittab`; this project
writes its own init (§2.2) that does five things and nothing else.

#dtable(
  columns: (auto, auto, 1fr),
  align: (left, left, left),
  ([Step], [Component], [What happens]),
  ([1], [GRUB], [Reads `/boot/vmlinuz` and hands off. Installed by `builder-core`'s image stage, unchanged from `distroless`.]),
  ([2], [Kernel], [Boots, mounts the ext4 rootfs read-write, execs `/sbin/init`. Built by `builder-core::stages::kernel` — the same generic stage `distroless` uses.]),
  ([3], [distro-init], [Our own PID 1 (§2.2). Mounts `/proc`, `/sys`, `/dev` (devtmpfs), and a fresh `tmpfs` at `/run`.]),
  ([4], [udevd, seatd, dbus-daemon], [Forked and exec'd, in that order. `distro-init` then runs `udevadm trigger` once — a *coldplug* that tells `udevd` about the devices devtmpfs already created before it started.]),
  ([5], [agetty ×2], [Forked on `tty1` (VGA console) and `ttyS0` (serial, for QEMU/headless use). Whichever exits gets respawned.]),
  ([6], [login], [`agetty` execs `/bin/login` once a username is typed. `login` reads `/etc/passwd` + `/etc/shadow` for real — the `root` account's shadow entry has an empty password field, so `login` skips the password prompt entirely rather than reimplementing BusyBox's separate no-password shortcut.]),
  ([7], [bash], [`login` execs the shell named in `/etc/passwd`. A real, interactive shell — not a script, not a fallback.]),
)

#callout(kind: "ok", "This is not a diagram — it was watched happening")[
  Every arrow above was reproduced live in QEMU with a scripted serial-console
  session, not inferred from source reading: `login: root` → no password
  prompt → `-bash-5.2#` → `dbus-send --system … ListNames` returns a real
  reply → `udevadm info --query=all --name=/dev/tty1` returns a populated
  device entry. See §8.4 for the harness.
]

== Why a custom init at all

BusyBox (which `distroless` uses) ships its own tiny init built in; nothing
glibc-based has an equivalent single binary. Rather than reach for systemd —
which would drag in `logind`, `udev`, `journald`, and a great deal more than
this system currently needs — `distro-init` is \~100 lines of Rust using the
`nix` crate:

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

Every daemon it starts would normally daemonize itself (fork, detach, exit
the parent) — `dbus-daemon --nofork` and `udevd` with no `-d` suppress that,
so they stay `distro-init`'s direct children and its `waitpid` loop actually
sees them exit if they crash.

= The Kernel

Step 2 of §2's boot table — the kernel itself — is built by
`builder-core::stages::kernel`, the same code `distroless` uses unchanged.
`distro`'s own `distro.toml` names a version and a download URL
(currently 6.17, from kernel.org) under `[kernel]`; `distro build-kernel`
fetches, configures, and compiles it.

== Where the starting config comes from

There are two starting points, chosen by whether `kernel.config_file` is
set in the config file:

- *No `config_file` (the default, and what `distro` currently uses).* Runs
  `make defconfig` — the kernel's own "reasonable defaults for this
  architecture" config — and then strips it down: every option belonging to
  one of the named feature packs below gets turned off, regardless of
  whether `defconfig` turned it on. Everything defconfig sets that *isn't*
  owned by a pack (PCI, ACPI, EFI boot, block/ATA/virtio, ext4/vfat, the
  console) is left exactly as defconfig set it — only the packs are
  deliberately stripped, not the boot-essential baseline.
- *`config_file` set.* A previously saved `.config` (produced by an
  interactive `make menuconfig` session — see below) is used as-is, only
  re-resolved via `make olddefconfig` in case it predates a kernel upgrade.
  Nothing gets stripped: whoever hand-picked that config already decided
  what they wanted.

Either way, whatever packs are named in `kernel.features` get turned back on
as the final step — so even a hand-edited `config_file` can still request
named packs on top of it, without needing to know the underlying
`CONFIG_*` symbols.

== The feature packs

Fourteen named bundles, each just a curated list of Kconfig options one
`kernel.features` entry away from being switched on as a unit:

#dtable(
  columns: (auto, 1fr),
  align: (left, left),
  ([Pack], [What it turns on]),
  ([`graphics`], [DRM/KMS graphics + fbdev console (i915, virtio-gpu, bochs, AGP) instead of plain VGA text.]),
  ([`sound`], [ALSA sound subsystem and the Intel HDA driver.]),
  ([`wireless`], [The Wi-Fi stack (cfg80211/mac80211) and rfkill.]),
  ([`hid-extras`], [Per-vendor HID quirk drivers (Sony, Samsung, Gyration, …) and the hiddev/hidraw userspace interfaces — generic USB HID keyboards/mice work without this.]),
  ([`legacy-nics`], [Dedicated Ethernet chipset drivers (Tigon3, Tulip, E100/E1000(E), Sky2, Forcedeth, 8139too, R8169) for real hardware — QEMU's virtio-net always works without this.]),
  ([`legacy-buses`], [PCMCIA/CardBus (Yenta) and legacy PATA chipset drivers — AHCI/virtio-blk always work without this.]),
  ([`network-fs`], [NFS (client + root-over-NFS), 9P, and autofs.]),
  ([`netfilter`], [Connection tracking, NAT, and iptables — only useful if this box routes or firewalls traffic.]),
  ([`security-extras`], [Disk quotas, POSIX ACLs, and SELinux.]),
  ([`iommu`], [AMD/Intel IOMMU support — only needed for PCI passthrough or running this as a virtualization host.]),
  ([`debug`], [Kernel debug instrumentation (Magic SysRq, schedstats, block-IO tracing, boot-param/entry debug) — useful while bringing up boot, dead weight once stable.]),
  ([`ia32-emulation`], [Run 32-bit x86 binaries on this 64-bit kernel.]),
  ([`iso9660`], [ISO9660/Joliet/zisofs filesystem support, for booting or mounting optical media images.]),
  ([`boot-logo`], [Framebuffer console + boot-time Linux logo (the stock penguin, or a custom image via `kernel.logo_file` — must be an 80×80, ASCII/P3 PPM with at most 224 colors, the kernel's own logo converter's hard requirement).]),
)

#callout(kind: "note", "distro's current kernel.features: none")[
  `distro.toml` sets no `kernel.features` at all — every pack above is off,
  which is why `distro`'s kernel today is the fully stripped `defconfig`
  baseline. Phase 3 (Mesa/graphics) will need at least `graphics` turned on;
  the original project roadmap expects most of these packs on for a real
  desktop eventually, none of that has happened yet.
]

== menuconfig's own menu tree, and what we actually touch

The fourteen packs above are this project's own grouping, not the kernel's.
Run `make menuconfig` against the fetched sources (via `menu-config`, §3.4)
and the kernel presents its *own* top-level menu structure — fourteen
menus of its own, as it happens, though not the same fourteen. Reading
straight from this kernel's own `Kconfig` files (not from memory), in the
order the config parser actually reaches them:

#dtable(
  columns: (auto, 1fr),
  align: (left, left),
  ([menuconfig's top-level menu], [Do any of our packs touch it?]),
  ([General setup], [No — left exactly as `defconfig` set it. Init system choice, cgroups, namespaces, `printk`, the core boot-essential baseline.]),
  ([Processor type and features], [No — CPU family, SMP, NUMA, `IA32_EMULATION`'s sibling 64-bit options all stay at `defconfig` defaults.]),
  ([Power management and ACPI options], [No — ACPI itself is part of the untouched boot-essential baseline the project's own docs call out explicitly.]),
  ([Bus options (PCI etc.)], [Yes — `legacy-buses` (PCMCIA/CardBus). Plain PCI itself is untouched.]),
  ([Binary Emulations], [Yes — `ia32-emulation` is literally the only option in this menu our packs name.]),
  ([Executable file formats], [No — ELF support stays at `defconfig` defaults.]),
  ([Memory Management options], [No — untouched.]),
  ([Networking support], [Yes — `wireless` (cfg80211/mac80211/rfkill) and `netfilter` (conntrack/NAT/iptables) both live here.]),
  ([Device Drivers], [Yes, the most — `graphics`, `sound`, `wireless` (the actual wireless-LAN drivers, as opposed to the stack above), `hid-extras`, `legacy-nics`, `legacy-buses` (PATA chipset drivers), `boot-logo`, and `iommu` all touch submenus here.]),
  ([File systems], [Yes — `network-fs` (NFS/9P/autofs), `iso9660`, and part of `security-extras` (disk quotas).]),
  ([Security options], [Yes — `security-extras` (SELinux).]),
  ([Cryptographic API], [No — untouched.]),
  ([Library routines], [No — untouched.]),
  ([Kernel hacking], [Yes — `debug`.]),
)

#callout(kind: "info", "Reading this the other way round")[
  Seven of the kernel's fourteen top-level menus have at least one pack
  reaching into them; the other seven — *General setup*, *Processor type
  and features*, *Power management and ACPI options*, *Executable file
  formats*, *Memory Management options*, *Cryptographic API*, and *Library
  routines* — the boot-essential core, plus crypto and the C library shims
  — are entirely untouched by this project's own config, left exactly as
  `defconfig` produced them. That's deliberate: the packs exist to strip
  *optional* hardware/feature surface, not to second-guess what a working
  x86_64 boot actually requires.
]

== What's exposed by each CLI

Everything in §3.1–3.2 lives in `builder-core::stages::kernel` as generic,
shared code — both distros' kernels are configured by the same functions.
What differs is how much of it each CLI actually puts a command in front
of, versus leaving as "edit the TOML file yourself":

#dtable(
  columns: (auto, auto, auto),
  align: (left, left, left),
  ([Capability], [`distroless`], [`distro`]),
  ([Fetch kernel source], [`fetch [--clean]`], [`fetch`]),
  ([Build the kernel], [`build-kernel`], [`build-kernel`]),
  ([Pick a version from kernel.org], [`resolve-kernel --channel <stable\|lts>`], [`resolve-kernel --channel <stable\|lts>`]),
  ([Interactive `make menuconfig`], [`menu-config --save-to <path>`], [`menu-config --save-to <path>`]),
  ([List the available feature packs], [`list-features`], [`list-features`]),
  ([Turn feature packs on], [`kernel.features = [...]` in the config file], [same: `kernel.features = [...]` in `distro.toml` — config-file level support is identical]),
  ([Custom boot logo], [`kernel.logo_file` in the config file], [same, `kernel.logo_file` in `distro.toml`]),
  ([Force any stage to rerun], [global `--force` flag], [global `--force` flag]),
  ([Interactive dashboard], [`tui` — a full terminal UI for every stage plus USB writing], [no equivalent]),
)

`resolve-kernel`, `menu-config`, and `list-features` are now wired up on
both sides. `menu-config` reuses `builder-core`'s `menuconfig()` directly
(it only reads `cfg.kernel_build_dir()` and writes the saved config to a
separate `--save-to` file, never touching the distro's own config file, so
it's safe unmodified against either `Config` type), and `list-features`
reuses the same `println!` loop over `FEATURE_PACKS` distroless's own
`list_features()` uses. `resolve-kernel` needed its own thin wrapper in
`distro/src/stages/kernel.rs`: `builder-core`'s version loads and saves a
whole `builder_core::config::Config`, which doesn't have `distro`'s
`bash`/`util_linux`/`shadow`/`seatd`/… sections — loading `distro.toml`
through it would fail to parse, and saving would silently drop everything
those functions don't know about. The kernel.org lookup itself was pulled
out into a shared `latest_release()` so both wrappers reuse the same
HTTP/JSON logic without duplicating it.

The only remaining CLI-surface difference in this table is `distroless`'s
`tui` — no equivalent exists for `distro`, and nothing in this section
claims otherwise.

= The Static Base

Five packages exist purely to get from a mounted rootfs to a real,
authenticated shell.

#callout(kind: "info", "coreutils, BusyBox, and uutils — three answers to the same question")[
  Every Unix system needs a basic set of file and text commands — `ls`,
  `cat`, `cp`, `mv`, `rm`, and so on. "Coreutils" is the generic name for
  that toolset, not one specific program. *GNU coreutils* is the
  implementation most desktop Linux distributions ship. *BusyBox* is a
  different, much older answer aimed at tiny/embedded systems: a single
  small C binary that crams coreutils *and* a shell, `init`, `mount`, `ps`,
  and dozens of other tools into one multi-call executable — this
  workspace's other distro, `distroless`, uses it. *uutils* is a third
  answer: a from-scratch reimplementation of just coreutils, in Rust. This
  project uses uutils instead of BusyBox for `distro` specifically because
  of the "prefer Rust alternatives to traditional tools" goal behind the
  whole `distro` crate — BusyBox would have worked technically, the same
  way it does for `distroless`.
]

The glibc equivalent of what BusyBox does as one binary is spread across
five separate upstream projects here, because glibc-land has no single
equivalent to reach for.

#dtable(
  columns: (auto, auto, auto, 1fr),
  align: (left, left, left, left),
  ([Package], [Version], [Provides], [Why it's there]),
  ([uutils/coreutils], [git `main`], [`ls`, `cat`, `cp`, …], [A Rust reimplementation of GNU coreutils — one multi-call binary, symlinked under every applet name. The built feature set is a curated subset (see §9), not the full set.]),
  ([bash], [5.2.37], [`/bin/bash`, `/bin/sh`], [The login shell named in `/etc/passwd`.]),
  ([util-linux], [2.41.2], [`agetty`, `mount`, `umount`], [Built with `--disable-all-programs` plus explicit `--enable-*` for just these three — util-linux ships dozens of tools, only these are needed.]),
  ([shadow-utils], [4.17.4], [`login`, `passwd`], [Real `/etc/passwd` + `/etc/shadow` authentication — not BusyBox's separate empty-password mechanism, genuine shadow-file semantics.]),
)

All five are *statically linked* — `--disable-shared --enable-static` at
configure time, `LDFLAGS=-all-static` at `make` time (plain `-static` alone
breaks configure's own compiler sanity check once libtool is involved). One
binary compiled, one binary copied into the rootfs, nothing else to track.
This is why they sit apart from everything in §5–7: dynamic linking doesn't
enter the picture until `dbus` (§5), which needs `libexpat` and isn't
meaningfully staticable.

#callout(kind: "trap", "A dispatch bug specific to this build")[
  uutils decides which applet `argv[0]` refers to by reading the kernel's
  `AT_EXECFN` auxval on non-musl Linux (hardening against `argv[0]`
  spoofing) instead of trusting `argv[0]` directly. On this build host — and
  inside this project's own kernel — `AT_EXECFN` comes back *empty*, so
  every applet invocation hit `"<unknown binary name>"` before dispatch even
  started. `distro/src/stages/fetch.rs` patches a fallback to `argv0` into
  uutils' `src/common/validation.rs` on every fetch, idempotently.
]

= The Seat & Session Layer

Three daemons exist to do the things a desktop session needs *before* any
compositor can run: let an unprivileged process touch `/dev/input` and
`/dev/dri`, pass messages between processes, and know what hardware is
plugged in. None of them are optional for Phase 3 — they're the floor a
compositor stands on, not decoration.

#callout(kind: "info", "What udev actually is")[
  The kernel's device drivers know about hardware, but they don't manage
  `/dev` themselves in any structured way — `devtmpfs` (already mounted by
  `distro-init`, §2) creates basic device nodes automatically as drivers
  load, and that's all it does. `udev` is the userspace daemon that listens
  for the kernel's own hardware-change announcements (sent over a netlink
  socket whenever something is plugged in, unplugged, or otherwise changes
  state), and in response creates or removes the matching `/dev` entries,
  applies permissions/rules, and maintains a live, queryable database of
  what hardware currently exists — the thing `udevadm info` reads from.
  Without it running, nothing gets notified when a device appears after
  boot, and nothing can ask "what's connected right now" in a structured
  way. `eudev` is a fork of `udev` that works without systemd, which this
  project needs since it uses `seatd` instead of `systemd-logind` (below) —
  ordinary `udev` is a systemd subproject these days.
]

#dtable(
  columns: (auto, auto, 1fr),
  align: (left, left, left),
  ([Package], [Version], [What it actually does]),
  ([seatd], [0.9.3], [Owns `/dev/input/*` and `/dev/dri/*` on behalf of whatever process asks nicely over its socket, so a compositor doesn't need to run as root to touch a keyboard or a GPU. Its own README says it "depends only on libc" — the systemd-`logind` alternative, chosen explicitly over pulling in systemd (see the callout below).]),
  ([dbus], [1.16.2], [The system message bus every desktop component uses to talk to every other one. Runs as `root` here (`-Ddbus_user=root`) — the rootfs has no unprivileged `messagebus` user yet to drop privileges to.]),
  ([eudev], [3.2.14], [A systemd-independent fork of `udev` (what Alpine, Void, and Gentoo use without systemd) — walks `/sys`, builds a device database, and exposes it as `libudev`. Exists in this pipeline for one specific reason: `libinput` (§6) hard-depends on `libudev`, and there is no way around that dependency.]),
)

#callout(kind: "info", "seatd instead of systemd — an open question, not a closed one")[
  Choosing `seatd` over systemd was a deliberate decision, made explicitly
  rather than defaulted into, to avoid pulling in `logind`, full `udev`,
  `journald`, and `networkd` for one seat-management socket. Whether `seatd`
  alone will actually satisfy COSMIC once Phase 5 needs a real session is
  still an open question — this system proves `seatd` starts and holds a
  seat, not that COSMIC will accept it as `logind`'s replacement.
]

`distro-init` starts all three (plus `udevd`, which is really this same
device-management job — see §2.1's table) and supervises them the same way
it supervises `agetty`. `dbus`'s socket lands at `/run/dbus/system_bus_socket`
and `distro-init` creates `/run/dbus` itself immediately after mounting a
fresh `tmpfs` at `/run`, since nothing else would.

= The Wayland-Core Libraries

Nothing in this section runs. Seven libraries are built and installed into
the rootfs, waiting for Phase 3's compositor to be the first thing that
actually links against any of them — verification at this layer is "builds
and installs cleanly against everything before it," not "does something
observable."

#dtable(
  columns: (auto, auto, 1fr),
  align: (left, left, left),
  ([Package], [Version], [What it's for]),
  ([wayland], [1.26.0], [The core wire-protocol libraries (client, server, cursor, EGL) and `wayland-scanner`, the code generator every later Wayland package runs at its own build time.]),
  ([wayland-protocols], [1.49], [The actual protocol definitions — `xdg-shell` and the rest — as XML. No library of its own, just data plus a pkg-config file.]),
  ([libxkbcommon], [1.12.4], [Turns "us, evdev, pc105" into the keymap tables a compositor hands to clients. X11 support is off (no `libxcb` — see §9); real keymap compilation needs the `xkeyboard-config` data package, also not built yet.]),
  ([pixman], [0.46.4], [Software rasterization — Mesa's fallback path, and some compositor-side operations not worth sending to the GPU.]),
  ([libdisplay-info], [0.4.0], [Parses a monitor's own EDID/DisplayID — how a compositor learns what resolutions and refresh rates a display actually supports.]),
  ([libevdev], [1.13.7], [Reads and writes raw evdev input-device events. `libinput`'s one mandatory dependency.]),
  ([libinput], [1.31.3], [Turns raw evdev events into the pointer/keyboard/touch/gesture events a compositor actually wants. `libwacom` (tablets) and `mtdev` (legacy multitouch) are both left out — see §9.]),
)

The dependency order between them is also the build order: `wayland` first
(nothing else here can build without `wayland-scanner`), then
`wayland-protocols` (needs `wayland-scanner` on `PATH`), then the rest, with
`libinput` last since it needs both `eudev`'s `libudev` (§5) and `libevdev`.

= The Graphics Stack

Phase 3's charter, per the project's own roadmap: get a graphics stack
working, scoped first to QEMU's own virtual GPU rather than real hardware.
Nothing in this section produces pixels on screen yet either — the
roadmap's own Phase 3 milestone is a minimal Wayland client actually
rendering something via virtio-gpu, which needs a compositor to host it
and is still ahead, not part of what's covered here — but this is the
piece that turns "a kernel that can talk to a GPU" into "a library a
compositor can actually call into."

#callout(kind: "info", "DRI, Gallium, EGL, GBM — what these actually are")[
  The kernel's DRM subsystem (already built — `CONFIG_DRM_VIRTIO_GPU`, part
  of the `graphics` feature pack, §3.2) owns the GPU at the lowest level:
  it hands out framebuffers and submits command buffers, but has no idea
  what "draw a triangle" means. *Mesa* is the userspace library that turns
  OpenGL/OpenGL ES calls into whatever that specific GPU actually
  understands. *Gallium* is Mesa's own internal plumbing for doing that
  once per GPU family instead of once per API — a "Gallium driver" is the
  translator for one specific GPU (or, here, one specific *virtual* one).
  *DRI* (Direct Rendering Infrastructure) is the convention by which an
  application actually finds and loads the right one at runtime. *EGL* is
  the glue between a window system (Wayland, here) and an OpenGL/GLES
  context — it's what a compositor or client actually links against, not
  Mesa's internals directly. *GBM* (Generic Buffer Management) is how a
  Wayland compositor allocates the actual pixel buffers it hands to the
  DRM/KMS display hardware — the piece that makes a rendered frame
  actually show up on a screen, as opposed to just existing in GPU memory.
]

#dtable(
  columns: (auto, auto, 1fr),
  align: (left, left, left),
  ([Package], [Version], [What it's for]),
  ([libdrm], [2.4.134], [The kernel-userspace ioctl wrapper every GPU-facing library, Mesa included, builds on. Every vendor-specific sub-library (Intel/AMD/nouveau/...) is disabled — virtio-gpu needs only libdrm's generic core.]),
  ([Mesa], [26.2.2], [`libEGL`, `libGLESv2`, `libgbm`, and the actual Gallium driver (`libgallium-26.2.2.so`) — built scoped to just the `virgl` (talks to QEMU's virtio-gpu/virgl backend, hardware-accelerated) and `softpipe` (software fallback) Gallium drivers.]),
)

Mesa's build answers a question the roadmap explicitly left open: whether
its virtio-gpu path needs LLVM (for `llvmpipe`, the LLVM-based software
rasterizer) or can go LLVM-free. It can — `llvmpipe` isn't in the driver
list above, `-Dllvm=disabled` is passed explicitly, and Mesa built and
installed cleanly regardless, since neither `virgl` nor `softpipe` needs
LLVM for anything. Vulkan is off entirely (`-Dvulkan-drivers=`, empty —
this milestone is OpenGL/GLES only), and so is GLX/X11
(`-Dplatforms=wayland`, `-Dglx=disabled`), consistent with the earlier
decision not to build `libxcb` (§9).

#callout(kind: "note", "A toolchain bump: meson via pip, not apt")[
  Mesa needs meson ≥ 1.4.0; Ubuntu 24.04's own `apt` package is 1.3.2.
  `build-toolchain` now also checks the installed meson's version and, if
  it's too old, runs `pip install --user --upgrade meson` — `sysroot_env`
  puts `~/.local/bin` ahead of `/usr/bin` on `PATH` explicitly for every
  build invocation from here on, rather than assuming the invoking shell
  already has it there.
]

#callout(kind: "trap", "One design limitation, three different packages")[
  `PKG_CONFIG_SYSROOT_DIR` (§8.3) is essential for letting one sysroot
  package's build find another's `-I`/`-L` flags correctly — but it applies
  uniformly to *everything* pkg-config resolves, with no way to tell "a
  package genuinely installed under our sysroot" apart from "a host build
  tool that happens to be installed for unrelated reasons." Phase 3 hit
  this three separate times, needing three different fixes — the same
  underlying limitation surfacing again, not the same bug recurring:

  - *Mesa's optional `spirv-tools` support* found this build host's own
    Homebrew-installed `SPIRV-Tools` (a nonstandard prefix,
    `/home/linuxbrew/...`), whose real include path then got rewritten
    into a sysroot location it was never installed under. Fixed by forcing
    `PKG_CONFIG=/usr/bin/pkg-config` in `sysroot_env` — the system pkg-config's
    own default search dirs never point outside a normal Ubuntu install,
    unlike Homebrew's own pkg-config wrapper, which bakes in Homebrew's
    paths as compiled-in defaults.
  - *libxkbcommon's optional `xkbregistry`* (XDG-style layout enumeration —
    not needed for keymap compilation itself) needed `libxml2`, which
    turned out to be genuinely installed at the *standard* system location
    too (`apt`'s `libxml2-dev`) — the `PKG_CONFIG` fix above doesn't help
    when the leak is a real system package, not a Homebrew one. Fixed by
    disabling the feature outright (`-Denable-xkbregistry=false`), since
    it isn't needed anyway.
  - *libdisplay-info's `hwdata` lookup* (a vendor-ID database it embeds at
    *build* time, nothing reads it at target runtime) isn't optional the
    way the other two are, so neither fix above applied. Its own
    `meson.build` already had a literal, correct fallback path for exactly
    this case (`/usr/share/hwdata/pnp.ids`) — unused because the
    `dependency('hwdata')` lookup "succeeds" first, then gets the same
    sysroot-mangling treatment. `distro/src/stages/fetch.rs` now patches
    this file on every fetch (idempotently, same pattern as the existing
    uutils `AT_EXECFN` patch, §4) to always take that branch.

  A fourth, unrelated bug surfaced in the same round of testing:
  `meson setup` refuses to reconfigure an already-configured `build/`
  directory, and fails outright — not just warns — if that directory was
  last configured by an older meson than is now on `PATH` (exactly what
  happened switching to pip's meson mid-project). `meson_build_and_install`
  now removes any existing `build/` before reconfiguring.
]

#callout(kind: "ok", "Verified")[
  The rebuilt image — kernel with `CONFIG_DRM_VIRTIO_GPU=y`, libdrm, and
  Mesa's `libEGL`/`libGLESv2`/`libgbm`/`libgallium-26.2.2.so` all present
  in the rootfs — still boots cleanly through the same checks as every
  earlier milestone: `login: root` → `-bash-5.2#` → `dbus-send` returns a
  real reply → `udevadm info` returns a populated device entry. No
  regression from Phase 2. Nothing yet *exercises* the new graphics
  libraries — that's the rest of Phase 3's own milestone (a minimal
  Wayland client rendering via virtio-gpu, which needs a compositor to
  host it, not yet built) — so this is "builds, installs, and boots
  cleanly," the same honest bar Phase 2's Wayland-core libraries table
  (§6) was held to.
]

== Weston, and the cairo chain that blocked it

Weston is the compositor Phase 3's milestone needs to host a client —
the one piece missing from "a library a compositor can call into" above
to "a Wayland client actually renders something." Building it turned out
to need six more from-source packages first, none of them optional:
weston's own `shared/meson.build` calls `dependency('cairo')` and
`dependency('libpng')` with no `required: false` — mandatory, not just a
demo-client extra — for its kiosk-shell window-decoration code.

#dtable(
  columns: (auto, auto, 1fr),
  align: (left, left, left),
  ([Package], [Version], [What it's for]),
  ([zlib], [1.3.2], [Compression — libpng's and cairo's own dependency.]),
  ([expat], [2.7.1], [XML parsing — fontconfig's config-file dependency.]),
  ([libpng], [1.6.44], [PNG images — weston's own unconditional dependency, and cairo's PNG surface backend.]),
  ([FreeType], [2.13.3], [Font rasterization. harfbuzz/bzip2/brotli/png support all disabled — none are built here, and none are needed for cairo's plain toy-font API.]),
  ([fontconfig], [2.15.0], [Font matching — FreeType's companion, needed for cairo's font backend to do anything useful. XML config via expat, not libxml2.]),
  ([cairo], [1.18.2], [2D graphics — weston's mandatory window-decoration dependency. X11/xcb/GL backends disabled (image-surface only, no `libxcb`, §9).]),
  ([Weston], [16.0.0], [The compositor itself — DRM backend, GL renderer, kiosk shell, `weston-simple-egl` only. Vulkan/X11/Xwayland/systemd/JPEG/WebP/LCMS2/docs/tests all disabled.]),
)

#callout(kind: "trap", "The same PKG_CONFIG_SYSROOT_DIR limitation, twice more — and one fix that backfired")[
  Two more instances of the same `PKG_CONFIG_SYSROOT_DIR` design limitation from earlier in this section, plus a lesson from
  trying to close the whole bug class at once instead of one instance at
  a time:

  - *weston's own `shared/meson.build`* unconditionally probes for
    `pango`/`pangocairo`/`fontconfig`/`glib-2.0` (all `required: false`,
    but with no meson option to skip the probe itself) — this build host
    has apt-installed `pango`/`glib` dev packages at genuine standard
    locations, so the probe "succeeds," and `cairo-util.c` ends up
    `#include <pango/pangocairo.h>` with no real header at the
    sysroot-rewritten path the compiler was given. None of pango, glib,
    harfbuzz, or fribidi are built here, and none are actually needed —
    weston's kiosk-shell decoration works fine on cairo's plain toy-font
    API. Fixed the same way as the earlier `hwdata` case: an idempotent source
    patch (`patch_disable_pango`) that forces the probe's own `if` to
    never fire.
  - *The tempting "real" fix* — set `PKG_CONFIG_LIBDIR` to only the
    sysroot's own pkgconfig directories, replacing pkg-config's built-in
    host search path entirely instead of just prepending to it — closes
    this whole bug class in one move, and was tried. It broke a
    different, *legitimate* case instead: `wayland-server` needs `libffi`,
    which — like glibc itself — this project deliberately never builds
    from source, relying on the host's copy the same way it already does
    for the runtime `.so` (§8.2's `HOST_DYNAMIC_LIBS` list). Excluding the
    host search path made that real dependency unfindable too. Reverted
    in favor of the established pattern: an explicit per-package disable
    each time an *unwanted* leak is actually hit, not a blanket
    restriction that can't distinguish "unwanted optional leak" from
    "deliberately host-provided dependency."
]

#callout(kind: "ok", "Verified")[
  `weston` and `weston-simple-egl` both build and install as genuine
  dynamically-linked ELF binaries in the sysroot. `readelf -d ... |
  grep NEEDED` on every new `.so` in the chain — `libweston-16`,
  `libexec_weston`, `drm-backend.so`, `gl-renderer.so`, `kiosk-shell.so`,
  and cairo/fontconfig/freetype/libpng/expat/zlib themselves — shows zero
  leftover host-only runtime dependency: everything resolves to either
  one of these seven packages, an already-built Phase 2/3 sysroot
  library, or glibc.

  Since this table, the actual milestone landed: `assemble-rootfs`
  picked up all seven packages automatically with zero code changes
  (they install into the same real sysroot the old pipeline already
  bulk-copies), and a real QEMU boot got weston all the way to a running
  compositor with `weston-simple-egl` connected and rendering —
  DRM/EGL/GL initialize using our own Mesa `softpipe` renderer, `libseat`
  grants session control, `libinput` configures all three input devices,
  and `kiosk-shell.so` loads. Getting there past this table surfaced two
  more real, previously-unnoticed gaps, both fixed and covered next: a
  missing `xkeyboard-config` data package, and eudev's own rules
  directory being silently wrong.
]

== xkeyboard-config, and a second PKG_CONFIG_SYSROOT_DIR casualty

Building weston is not the same as running it. The first real QEMU boot
attempt failed immediately: `xkbcommon: ERROR: failed to add default
include path /usr/share/X11/xkb` / `failed to create XKB context` — the
already-documented gap (§9) that `xkeyboard-config`, the keyboard
layout/rules *data* package (no C code, nothing links against it — it's
what `libxkbcommon` looks for at *runtime*), was never built. Adding it
as an eighth buildpack fixed this cleanly.

The next boot attempt got further — DRM, EGL, and GL all initialized
successfully against our own Mesa build — then failed differently: every
input device (`Power Button`, `AT Translated Set 2 keyboard`,
`ImExPS/2 Generic Explorer Mouse`) was rejected as "not tagged as
supported input device," and weston aborted with "failed to create
compositor backend." `strings` on the built `udevd` binary explained
why: this build host has systemd's own `udev.pc` installed (apt's
`systemd-dev` package), which eudev's `configure` auto-detects its
rules directory from — and `PKG_CONFIG_SYSROOT_DIR` rewrote that
variable into a *literal build-machine absolute path*
(`.../build-distro/sysroot/usr/lib/x86_64-linux-gnu/udev/rules.d`),
baked into `udevd` as a compiled-in constant. The same
`wayland_scanner`-variable bug class documented earlier in this
section — except silent instead of an immediate build failure, since
that path genuinely exists *on the build machine*, and only breaks once
the rootfs is booted as an independent image where it doesn't exist at
all. `build_eudev` now passes `--with-rootlibexecdir=/usr/lib/udev`
explicitly, bypassing the sysroot-mangled auto-detection.

#callout(kind: "ok", "Verified — the actual milestone")[
  With both fixes, a real QEMU boot: `udevd` tags every input device
  correctly ("is tagged by udev as: Keyboard"/"Mouse"), `weston`
  (launched with a minimal `weston.ini` selecting `kiosk-shell.so` —
  without one, weston defaults to `desktop-shell.so`, which was never
  built) initializes DRM, EGL/GL via our own Mesa `softpipe` Gallium
  driver, libinput, and the kiosk shell, and opens a Wayland socket.
  `weston-simple-egl`, run against that socket, connects and runs
  without error or crash — the actual roadmap milestone: *a minimal
  Wayland client renders via virtio-gpu inside QEMU.* The new buildpacks
  and the `eudev` fix needed no CLI/rootfs wiring beyond the existing
  `assemble-rootfs`/`make-image` — everything lands in the one shared
  sysroot both the old pipeline and the new buildpacks write into.
]

= How It's Actually Built

== The config file

`distro.toml` is not one monolithic struct. `distro`'s `Config` composes the
pieces that are genuinely identical to `distroless` (`KernelConfig`,
`ImageConfig`, `UutilsConfig`, all from `builder-core`) with a `[section]`
per package in §4–7 — each just a `version` and a source `url`, plus one
`build_dir()` helper method per package computing exactly where its tarball
extracts to.

== The pipeline, stage by stage

#codepanel(title: "distro's CLI surface (distro/src/cli.rs)")[
```
distro fetch                    # download + extract every source tarball
distro build-toolchain          # apt-get the host build tools (once)
distro build-kernel              # builder-core, unchanged from distroless
distro build-userland            # every package in §4 through §7
distro assemble-rootfs           # merge it all into build-distro/rootfs
distro make-image                # partition + GRUB + write the disk image
distro test-qemu [--window]      # boot it
distro write-usb --device <dev>  # dd to real hardware (destructive)
distro all                       # the whole pipeline, in order
```
]

`build-userland` is where §4–7's packages actually compile — each package
gets its own `build_<name>` function in `distro/src/stages/userland.rs`,
called in dependency order. `assemble-rootfs` then builds the actual root
filesystem tree: coreutils and its applet symlinks, bash, the §4 static
binaries, the §5–7 dynamic ones (via the sysroot, below), the host's own
`libc.so.6`/`libexpat.so.1`/`libm.so.6`/`libgcc_s.so.1`/`libstdc++.so.6`/
`libz.so.1`/`libzstd.so.1`/`libffi.so.8` (the list has grown package by
package — `libffi` turned out to be a latent gap since Phase 2's
`libwayland-client`, just never caught until Mesa's EGL exercised it too)
and the dynamic linker (confirmed via `ld-linux-x86-64.so.2 --help` to
already be on glibc's default search path here — no `ldconfig` step
needed), `distro-init` itself as `/sbin/init`, and `/etc/passwd`+`/etc/shadow`.

== The sysroot: how packages in §5–7 find each other

Phase 1's five packages never needed each other at build time — each just
needed the host's gcc. §5–7's packages do: `wayland-protocols` needs
`wayland-scanner` on `PATH` at its own build time, and `libinput` needs
`eudev`'s installed `libudev.pc` to link against `libudev` at all. Every
package in §5–7 is therefore built with `--prefix=/usr` (its normal, final,
"as if genuinely installed" prefix) and installed with
`DESTDIR=<build-distro/sysroot>` — files physically land under the sysroot,
but the package's own compiled-in idea of its prefix stays `/usr`, which
matters: `dbus`, for instance, looks up its own config file relative to
whatever prefix it was *actually built with*, at its own runtime, on the
real target — not at build time. `PKG_CONFIG_SYSROOT_DIR`, a mechanism
pkg-config itself provides, then rewrites the `-I`/`-L` paths a later
package's build sees from `/usr/...` to the sysroot's real, on-disk
`<sysroot>/usr/...`. `assemble-rootfs` copies that whole sysroot tree into
the final rootfs with one `cp -a`.

#callout(kind: "trap", "One package breaks this pattern, on purpose")[
  `wayland` alone is built differently: with a real, absolute,
  on-disk `--prefix=<sysroot>/usr` and installed directly, no `DESTDIR`.
  `wayland-scanner`'s own path is baked into `wayland`'s `.pc` file as a
  *custom* variable (`wayland_scanner=${bindir}/wayland-scanner`) that —
  unlike ordinary `Cflags`/`Libs` — `PKG_CONFIG_SYSROOT_DIR` does not
  rewrite, and `wayland-protocols`' build executes whatever path that
  variable names, directly, as part of its own build. With `--prefix=/usr`
  that path would read the literal string `/usr/bin/wayland-scanner`, which
  does not exist anywhere on the host. This is safe specifically for
  `wayland` because none of its own *shared libraries* do a prefix-derived
  runtime lookup the way `dbus`/`eudev`'s daemons do.

  An earlier version got this backwards — every package used the absolute
  sysroot prefix, not just `wayland` — and it booted, but `dbus-daemon`
  crash-looped: `distro-init` logged `"dbus-daemon exited, respawning"` on
  repeat, because `dbus` had baked in a literal build-machine path
  (`.../build-distro/sysroot/usr/share/dbus-1/system.conf`) as its config
  search location. Found by an actual QEMU boot, not by review.
]

== How every claim in this report was checked

A Python harness opens a PTY (`pty.openpty()`, with `TIOCSWINSZ` set — QEMU's
serial console renders nothing at a 0×0 terminal size), launches
`qemu-system-x86_64` with the built image over `virtio` and OVMF for UEFI
boot, and drives the console with a small state machine keyed on regexes
over the accumulated output — `login:`, a shell prompt, a sentinel string
echoed after each command — rather than fixed sleeps.

#callout(kind: "trap", "A build-pipeline bug this same method caught")[
  `assemble-rootfs` once failed outright with "No such file or directory"
  copying the freshly built `distro-init` binary. Cause: this build host
  sets `CARGO_TARGET_DIR` to a shared cache outside the repo, which `cargo`
  honors — so the binary was never at the hardcoded `target/…` path the
  rootfs-assembly code assumed. Fixed by reading `CARGO_TARGET_DIR` at the
  same call site cargo itself does.
]

== A buildpack architecture, in progress

Every package above is defined the same way: a `{version, url}` config
struct plus a `build_dir()` method in `Config`, a fetch call in
`fetch.rs`, a `build_<name>` function in `userland.rs`, and — for the
static packages (§4) — an `install_<name>` function in `rootfs.rs`. That
smearing of one package's identity across four files stopped scaling once
`distro` passed \~17 packages, so a new foundational crate,
`buildpack-core`, defines a `Buildpack` trait instead: one self-contained
value per package, declaring its own source, dependencies, build
instructions, and build outputs, plus a description for an eventual
generic TUI (today only `distroless` has one, hand-maintained separately
from its own stage functions).

#dtable(
  columns: (auto, 1fr),
  ([Crate], [Role]),
  ([`buildpack-core`], [The trait itself, `graph::topo_order` (a real topological sort over each package's declared dependencies, replacing a hand-maintained call sequence), and the shared meson/autotools/cargo build helpers.]),
  ([`buildpacks`], [One implementation per package, in one crate regardless of which distro(s) end up using it — which distro consumes a package isn't a meaningful axis to split crates on.]),
)

Eleven packages are implemented and verified against the real
`distro.toml` and the real `build-distro/sysroot` this way so far: the
kernel (its `FEATURE_PACKS`, §3.2, kept as its own internal mechanism
rather than becoming buildpacks themselves — they have no source or
build step of their own), util-linux, Mesa, and the seven-package cairo
chain (including `xkeyboard-config`) above. *All eleven* are now what
`distro`'s real CLI actually calls, not proof-of-concept duplicates
sitting alongside working code: `stages/buildpacks.rs` builds the
cairo-chain packages (no old-pipeline equivalent at all — pure
addition) plus util-linux and Mesa (cut over — their old
`distro/src/stages/userland.rs` implementations are deleted) in
`topo_order`, right after the old pipeline's own remaining packages;
`build-kernel`/`menu-config`/`list-features` call the kernel buildpack
directly, its old `builder_core`-wrapper calls removed too. One command
(`distro build-userland`, or `distro all`) builds all 25 packages —
including the once-separate `cargo run -p buildpacks --example
weston_chain` step, now gone entirely.

Cutting kernel/util-linux/Mesa over needed one real design fix:
`topo_order` used to hard-error on any declared dependency id not
present in the registry it was given, but Mesa's *real* dependencies
(libdrm, wayland, libxkbcommon, pixman, `libdisplay_info`, libinput)
aren't buildpacks yet — building a deliberate subset of a larger
pipeline is the normal case, not a registration bug, so an unregistered
dependency id is now silently treated as already-satisfied rather than
an error. It also needed each of the three cutover packages to get its
own `BuildCtx` pointed at its OLD on-disk build location
(`build_dir/kernel`, `build_dir/util-linux`, `build_dir/mesa` — not the
shared `build_dir/sources` every other buildpack uses), so the
already-built kernel/util-linux/Mesa trees already on disk were
recognized as-is instead of the cutover triggering a redundant rebuild
— a kernel rebuild in particular being far too expensive to redo
needlessly. Verified: every one of the 25 packages reports "already
exists" on a rebuild, zero wasted work, and a full QEMU regression
boot (login, `dbus-send`, `udevadm`) still passes exactly as before
the cutover. This needed no rootfs-side wiring for the cairo chain at
all: `assemble-rootfs`'s existing `install_sysroot` (`cp -a` of the
whole shared sysroot) already picks up whatever landed there,
regardless of which code built it — exactly how Weston ended up
actually running inside QEMU (§7.1). util-linux, being a
`StaticArtifacts` package, *did* need `rootfs.rs`'s
`install_util_linux` rewritten to call the buildpack's own
`outputs()` instead of duplicating the full/minimal branch and
ELF-scan logic locally — now one implementation, not two.

The remaining ~16 packages (bash, shadow, seatd, dbus, eudev, wayland,
wayland-protocols, libxkbcommon, pixman, libdisplay-info, libevdev,
libinput, libdrm, uutils, plus `distroless`'s own busybox) stay on the
old pipeline for now, migrating on their own schedule.

#callout(kind: "trap", "A regression from trying to fix a bug class, not an instance")[
  `sysroot_env` briefly set `PKG_CONFIG_LIBDIR` (which replaces
  pkg-config's own default search path, rather than just prepending to
  it) to close the whole "unwanted host package leaks into a sysroot
  build" bug class in one move. It broke a real case instead —
  `wayland-server`'s legitimate, deliberately-host-provided `libffi`
  dependency became unfindable — and was reverted in favor of the
  established pattern: an explicit per-package fix each time an actual
  unwanted leak is hit, not a blanket restriction that can't tell "leak"
  from "genuine host dependency" apart. See the weston/cairo section
  above for the case that prompted trying it.
]

= What Isn't Part of the Picture Yet

#spec(
  ("Phase 3", [*Done* — §7.1's actual milestone verified in a real QEMU boot: `weston-simple-egl` connects to a running Weston compositor and renders via virtio-gpu/Mesa `softpipe`. Not yet done, and left for later: making this happen automatically at boot (today it's started by hand from a login shell with a hand-written `weston.ini`, not a `distro-init` service) — a smaller, well-understood remaining step, not a new unknown.]),
  ("Phase 4", [`rustup`/`cargo` on-target, plus a curated Rust-CLI-tools suite (ripgrep, bat, eza, …).]),
  ("Phase 5", [COSMIC itself — `cosmic-comp`, `cosmic-session`, `cosmic-panel`, `cosmic-greeter`, minimal subset first.]),
  ("Phase 6", [Expand: more COSMIC components, real GPU drivers beyond `virtio-gpu`, audio, networking UI.]),
)

And three deliberate gaps in what's already built, worth knowing about
rather than discovering later:

- uutils' built feature set does not include `grep` or `sed`, despite both
  names appearing in `rootfs.rs`'s applet-symlink list — they were never
  part of coreutils' scope upstream in the first place. The symlinks exist
  but dangle. Not yet fixed.
- `libxcb` was never built. Its only real consumer on a Wayland-native
  target is XWayland compatibility, not currently planned — three more
  from-source packages (`libxcb`, `libXau`, `libXdmcp`) not worth building
  for a checklist item with no near-term user.
- `mtdev` (legacy multitouch) and `libwacom` (tablet identification) were
  both left out of `libinput` — niche hardware support, easy to add back
  later if a real device needs it.
