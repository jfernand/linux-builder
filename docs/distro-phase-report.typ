#import "isss-template.typ": *

// A vertical stack of layer boxes, bottom-most item first in the call —
// rendered top-down (last item on top), connected by down-arrows, the
// top layer picked out in amber. Each item is (label, note) or
// (label, none).
#let layerstack(..items) = {
  let list = items.pos().rev()
  let parts = ()
  for (i, it) in list.enumerate() {
    let (lbl, note) = it
    parts.push(block(
      width: 82%, fill: if i == 0 { brand-primary } else { c-white },
      stroke: 1pt + fg-primary, inset: (x: 10pt, y: 7pt), radius: 1pt,
    )[
      #text(weight: 700, size: 8.4pt, fill: fg-primary)[#lbl]
      #if note != none [ #text(size: 7.2pt, fill: fg-secondary)[— #note]]
    ])
    if i < list.len() - 1 {
      parts.push(align(center)[#text(size: 11pt, fill: fg-muted)[↓]])
    }
  }
  block(width: 100%, above: 16pt, below: 16pt)[
    #align(center)[
      #stack(dir: ttb, spacing: 3pt, ..parts)
    ]
  ]
}

// A left-to-right chain of short boxes joined by arrows — a build/data
// flow, read in call order.
#let flow(..steps) = {
  let items = steps.pos()
  let parts = ()
  for (i, it) in items.enumerate() {
    parts.push(block(
      fill: c-white, stroke: 1pt + fg-primary,
      inset: (x: 7pt, y: 5pt), radius: 1pt,
    )[#text(size: 7.4pt, weight: 600, fill: fg-primary)[#it]])
    if i < items.len() - 1 {
      parts.push(align(horizon)[#text(size: 10pt, fill: fg-muted)[→]])
    }
  }
  block(width: 100%, above: 14pt, below: 14pt)[
    #align(center)[
      #stack(dir: ltr, spacing: 6pt, ..parts)
    ]
  ]
}

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

#layerstack(
  ("Bootloader", "GRUB or similar — finds and loads the kernel"),
  ("Kernel", "vmlinuz — hardware, memory, mounts root"),
  ("Userspace", "init (PID 1) and everything it starts"),
)

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

== Firmware and the bootloader: getting to step 1

A bootloader doesn't run because the disk is bootable in some magical
sense — the machine's own firmware is what decides to run it, and *how*
differs by firmware type:

#dtable(
  columns: (auto, 1fr),
  ([Firmware], [How it finds something to run]),
  ([Legacy BIOS], [Reads the first 512-byte sector of the boot disk (the *MBR*) and executes it directly as code — that sector is normally just a small stub whose entire job is to load a larger second stage from elsewhere on disk, since 512 bytes isn't room for a real bootloader.]),
  ([UEFI], [Reads a `FAT32`-formatted partition specifically marked as the *EFI System Partition* (ESP) and executes a `.efi` file from it directly — a real PE-format executable, not a boot-sector stub. Which `.efi` file to run is normally recorded in the firmware's own NVRAM as a named boot entry, *or* — if nothing is registered — UEFI falls back to a fixed, well-known path on the ESP: `\EFI\BOOT\BOOTX64.EFI`.]),
)

GRUB, install and all, is itself split the same way BIOS-era bootloaders
were: a minimal loader the firmware hands off to, which then loads GRUB's
*real* logic — including reading `grub.cfg` — from wherever it was
actually installed. Installing it with `--removable` specifically targets
UEFI's fallback path above rather than registering an NVRAM entry, so the
resulting disk boots correctly on *any* UEFI firmware, unmodified NVRAM
included — the same property that makes a "boot from USB" disk work on a
machine that's never seen it before.

`grub.cfg` itself does two things: `search --fs-uuid` locates a
filesystem by its on-disk UUID and sets `$root` to wherever GRUB finds
it (independent of `/dev/sdX`-style naming, which can differ between
boots), and `linux` then loads the kernel image from that filesystem and
sets its command line — critically, `root=PARTUUID=...`, a *different*
identifier from the filesystem UUID `search` just used: the kernel
itself only understands `PARTUUID=` natively for locating its own root
partition at boot, not the filesystem UUID GRUB found it by.

== Telling where you actually booted from

None of the above has to be taken on faith — a running system can be
asked directly what booted it, from most to least specific:

#dtable(
  columns: (auto, 1fr),
  ([Check], [What it tells you]),
  ([`cat /proc/cmdline`], [The exact kernel command line GRUB set — including the literal `root=PARTUUID=...` (or `UUID=...`) value the kernel used to find its own root partition.]),
  ([`findmnt /` or `cat /proc/mounts`], [The actual device node (`/dev/vda2`, say) the kernel resolved that identifier to and mounted — after resolution, not the identifier itself.]),
  ([`dmesg | grep -i "mounted root"`], [The kernel's own boot-time log line recording which device and filesystem type it mounted as root — a permanent record of what happened, not just what was asked for.]),
  ([`blkid`], [Cross-references a `PARTUUID`/`UUID` from `/proc/cmdline` against every real device on the system, useful for going the other direction — "which physical partition does this identifier actually name?"]),
  ([`[ -d /sys/firmware/efi ]`], [Whether this boot went through UEFI at all — present only if it did, absent on a legacy BIOS boot. A different firmware path entirely (above), not just a detail of which partition was chosen.]),
)

== From a loaded image to a running kernel

Handing off to the kernel isn't handing off to a fully capable OS yet —
the image GRUB loaded is compressed, and unpacks itself first via a
small stub linked into the front of the file. Once running for real, the
kernel walks the memory map the firmware handed it, brings up its own
core subsystems (scheduler, memory management, the device model
underlying §3), and probes for hardware, loading whatever drivers are
built directly into this kernel image as it finds matching devices.

Some systems need a userspace-driven step *before* any of this can even
locate a real root filesystem — a RAID array to assemble, an
encrypted volume to unlock, or a storage driver too specialized to be
built into the kernel image itself, handled by a temporary *initramfs*:
a small, self-contained root filesystem, embedded alongside the kernel
image, whose only job is running just enough userspace to make the real
root filesystem reachable before handing off to it and being discarded.
A kernel built with every storage driver it needs compiled directly in
— true for both `distro` and `distroless`, §9–10 — has no such gap to
close, and mounts its real root filesystem directly, with no initramfs
stage at all.

== The pseudo-filesystems init mounts, and why

None of `/proc`, `/sys`, `/dev`, or `/run` hold real files on disk —
each is a *pseudo-filesystem*, generated live by the kernel (or, for
`/dev`, populated live as described in §3) rather than read from a block
device, and each has to be mounted explicitly by init before anything
depending on it can work:

#dtable(
  columns: (auto, 1fr),
  ([Mount], [What it actually is]),
  ([`/proc`], [A live view of kernel and per-process state — one directory per running PID, plus kernel-wide tunables and statistics. Standard tools (`ps`, `top`, and plenty of libraries) read straight from here rather than through some other API.]),
  ([`/sys`], [`sysfs` — the kernel's device/driver model, described in full in §3.1.]),
  ([`/dev`], [`devtmpfs` — where device nodes actually live, populated as drivers bind to hardware, refined by udev's rules pass (§3).]),
  ([`/run`], [An ordinary `tmpfs` — real files, but backed by RAM, not disk, and empty again on every boot. Where sockets, PID files, and other runtime-only state belong precisely *because* nothing should expect it to survive a reboot.]),
)

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
  ([A purpose-written init], [Nothing says init has to be either of the above. A minimal init can be a short, direct program: mount what's needed, fork a fixed set of children, wait for any of them to exit, fork it again. §10.1.1's `distro-init` is exactly this — about 100 lines.]),
)

None of these is "more correct" than the others — they're different
trade-offs between flexibility (systemd can express arbitrary service
dependency graphs) and simplicity (a fixed list of children is easy to
read in full and reason about completely).

= Device Management: udev

The kernel's device drivers know about hardware the moment it's
detected, but "knowing about it" and "userspace can actually use it" are
two different things. Getting from one to the other is a genuine
pipeline, not a single step.

#flow("Device appears", "kernel emits uevent", "udevd runs rules", "/dev node fixed up", "database updated")

== sysfs: the kernel's hardware model, as a filesystem

Every device, bus, and driver the kernel currently knows about is
represented internally as a *kobject*, and the whole tree of them is
exposed to userspace as `sysfs`, mounted at `/sys`. This is metadata and
control, not device access: a directory per device, files inside it for
that device's attributes (`/sys/class/net/eth0/address`, say), and
symlinks expressing the relationships between devices, drivers, and
buses. Nothing in `/sys` is a device node you can `open()` and read
bytes from — that's a separate, second thing, over in `/dev`.

`devtmpfs`, mounted early in boot, is what actually populates `/dev`: as
each driver binds to a device, the kernel itself creates the matching
device node there (a special file recording a major/minor number pair,
not real file content) with a kernel-chosen name and default,
usually-too-permissive ownership. That's the entire extent of what the
kernel does on its own — correct device nodes exist, with root-only or
otherwise generic permissions, no stable naming beyond whatever order
drivers happened to probe in, and no way for anything to ask "what's
connected right now" after the fact.

== uevents: how the kernel announces a change

Every time a device is added, removed, or changes state, the kernel
emits a *uevent* — a small message, broadcast over a dedicated netlink
socket (`NETLINK_KOBJECT_UEVENT`), carrying `KEY=value` pairs:
`ACTION=add`, the device's `/sys` path, its `SUBSYSTEM` (`input`,
`block`, `drm`, …), major/minor numbers, and a monotonically increasing
sequence number so ordering survives even if events are handled out of
strict arrival order. `udevd` is a long-running process that does nothing
but sit on that socket and react.

== Rules: deciding what a device actually gets

Reacting to a uevent means running it through *udev rules* — plain-text
files (`/usr/lib/udev/rules.d/*.rules`, with `/etc/udev/rules.d`
available to override them) matched top to bottom against that device's
properties:

#dtable(
  columns: (auto, 1fr),
  ([Directive], [What it matches or does]),
  ([`SUBSYSTEM==`, `KERNEL==`], [Match against the uevent's own subsystem and kernel-assigned device name.]),
  ([`ATTR{name}==`], [Match a `sysfs` attribute's value — a device's vendor/product ID, for instance.]),
  ([`ENV{KEY}==`], [Match a property attached by an *earlier* rule or a previous pass — rule evaluation is cumulative, not one-shot.]),
  ([`SYMLINK+=`], [Add a stable, descriptive symlink to the node `devtmpfs` already created — `/dev/disk/by-id/...`, `/dev/input/by-path/...` — so nothing has to hardcode a `sdX`/`eventN` name that can change between boots.]),
  ([`MODE=`, `OWNER=`, `GROUP=`], [Fix up the permissions `devtmpfs`'s default was never going to get right for every device class.]),
  ([`TAG+=`], [Attach a label other tools query for later — `seatd` (§4) and `libinput` (§10.3.3) both rely on devices being tagged consistently to recognize what they are.]),
  ([`RUN+=`], [Run an external program as part of handling this event.]),
)

`udevd` forks a worker process per device to evaluate its rules — in
parallel across independent devices, but serialized for any one device
so a rename can't race a later rule that depends on the new name. Since
`devtmpfs` already created the node, most of what a rule does is refine
it: rename or symlink it, correct its permissions, tag it — actual
`mknod` only falls to udev itself on a system with no `devtmpfs` at all.

== Coldplug vs. hotplug

A uevent is a live, one-time broadcast — anything not yet listening when
it fires simply never sees it. That's exactly the situation at boot:
`devtmpfs` creates nodes for every device already present *before*
`udevd` itself has started, so none of them ever produced a uevent
`udevd` was around to catch. *Coldplug* is the fix: `udevadm trigger`
walks the current `/sys` tree and synthesizes a uevent for every device
already there, run through the exact same rules as a live hotplug event.
Skipping this step boots to a system where `/dev` nodes exist (courtesy
of `devtmpfs`) but none of them have been renamed, symlinked, permission-
corrected, or tagged — which is why `distro-init` (§10.1.1) runs
`udevadm trigger` once, immediately after starting `udevd`.

== The device database

Beyond the rules pass, `udevd` maintains a live database under
`/run/udev/data/`, one entry per device, holding everything a rules pass
determined about it — its tags, its properties, its stable names. This
is what `udevadm info --query=all --name=<path>` actually reads, and
what lets other components ask "what kind of device is this" without
re-deriving it themselves; `libinput` (§10.3.3) and `seatd`'s own device
filtering (§4) both depend on this database being populated and current,
not just on the raw `/dev` node existing.

Without something doing this whole job — sysfs walk, uevent listener,
rules engine, database — a system boots to device nodes with no stable
names, no correct permissions beyond whatever `devtmpfs` guessed, and no
way to notice a device that appears after boot at all. `udev` is the
canonical implementation; `eudev` is a fork that does the same job
without depending on systemd (ordinary `udev` is a systemd subproject
today) — the same rule syntax, the same netlink/`sysfs`/database
mechanics, just packaged to build and run standalone. It's the device-
management answer this workspace actually uses (§9, §10).

= Seat & Session Management

A Wayland compositor needs exclusive, arbitrated access to raw input
devices (`/dev/input/*`) and the GPU (`/dev/dri/*`) — but running it as
root just to get that access is the wrong trade-off, and letting an
unprivileged process `open()` those nodes directly hits real permission
walls even if it wanted to. *Seat management* is the piece that solves
this — and it turns out to bundle together several related problems, not
just one.

#flow("Compositor", "libseat", "seatd", "/dev/input, /dev/dri")

== What a "seat" and a "session" actually are

A *seat* is a bundle of hardware usable by one person at a time — at
minimum a display, a keyboard, a pointer. Most machines have exactly one
seat; a *multi-seat* system has several independent bundles (separate
GPUs, separate input devices) so more than one person can use the same
physical machine simultaneously, each with their own login. A *session*
is the lifetime of one logged-in user's occupancy of a seat — tracked as
its own thing (not just "a process is running") because a seat's devices
need to change hands cleanly: when a session stops being the
foreground one, whatever was using its keyboard/GPU access needs to
actually lose that access, not just keep holding a file descriptor open
against a display no one can see.

== Why this needs a broker at all

Two problems, not one, have to be solved before a compositor can safely
touch `/dev/input/*` and `/dev/dri/*` as an ordinary user:

#dtable(
  columns: (auto, 1fr),
  ([Problem], [What has to happen]),
  ([Permission], [`/dev/input/*` and `/dev/dri/*` are root- or group-restricted by default (§3's `udev` rules decide exactly how). An unprivileged compositor process has no path to opening them directly.]),
  ([Arbitration], [Switching virtual terminals (`Ctrl`+`Alt`+`F2`, say) has to *revoke* the outgoing session's device access and hand it to the incoming one — two compositors racing for the same GPU is a crash, not a feature. The kernel's own VT subsystem signals this switch; something has to react to it and actually pause/resume the affected session's access.]),
)

A seat daemon solves both at once: it holds the real, privileged file
descriptors open itself, and hands duplicated, access-controlled
descriptors to whichever session currently owns the seat over a small
IPC protocol — reacting to VT-switch signals by revoking and reassigning
them, so a compositor never has to implement VT arbitration itself, and
never needs elevated privileges to begin with.

== Three implementations, and an abstraction layer over them

#dtable(
  columns: (auto, 1fr),
  ([Implementation], [What it actually is]),
  ([`systemd-logind`], [Part of systemd proper. Exposes session/seat state over a D-Bus API (`org.freedesktop.login1`) that a lot of desktop software — not just compositors — queries directly: polkit's authentication prompts, GNOME/KDE's own session management, suspend/inhibit locks all go through it. The default on most desktop distributions specifically because that D-Bus API is a de facto standard other software already expects.]),
  ([`elogind`], [A standalone extraction of `logind` — the same `org.freedesktop.login1` D-Bus API, the same session/seat/VT-switch behavior, packaged to build and run without the rest of systemd. Exists specifically so non-systemd distributions can still run desktop software that was written assuming `logind`'s D-Bus interface exists.]),
  ([`seatd`], [A smaller, purpose-built daemon: no D-Bus, no session-management API beyond the one job — a minimal socket protocol for "give me this device" / "you no longer have this device." Its own documentation describes it as depending only on libc. This workspace's choice (§9, §10), made explicitly to avoid pulling in `logind`, full `udev`, `journald`, and `networkd` for one seat-management socket.]),
)

Compositors that want to support more than one of these without three
separate code paths link against `libseat` instead of any daemon's
protocol directly — a small client library with backends for both
`seatd` and `logind`/`elogind`'s D-Bus API, so the same compositor binary
works against whichever is actually running on a given system. Weston
(§10) is built this way.

#callout(kind: "info", "seatd instead of systemd-logind — an open question, not a closed one")[
  Choosing `seatd` was deliberate, to avoid the rest of what a full
  systemd install brings in — but `seatd` alone provides none of
  `logind`'s D-Bus session-management API, only device arbitration.
  Whether that's sufficient once a real desktop session (COSMIC, §11)
  needs the broader session-management surface polkit and friends expect
  is still an open question this system hasn't had to answer yet.
]

= Userland Basics: Coreutils, Shells, and Logging In

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

== The multi-call binary technique

BusyBox and uutils both work the same way: one compiled binary contains
every applet's code, and a symlink exists for every command name it
supports — `/bin/ls`, `/bin/cp`, and so on — all pointing at that one
file. When the kernel `exec()`s a program reached through a symlink, it
resolves the symlink to find the actual file to run, but still passes
the *path used to invoke it* (`/bin/ls`, not the multi-call binary's own
name) as `argv[0]`. The binary's own `main` reads that back, strips it
down to a bare command name, looks it up in an internal dispatch table,
and jumps straight into that applet's implementation — one process
image, one `exec()`, no subprocess spawned to "really" run `ls`.

This is why the applet name in `argv[0]` has to be trustworthy for
dispatch to work at all. Ordinarily it is — but it's technically just a
string the caller chose, not something the kernel guarantees matches
reality, and at least one context on this system's own build host turned
out to supply an empty value instead of the expected path, which a naive
"trust `argv[0]`" dispatch would silently mishandle. Some tools defend
against `argv[0]` spoofing entirely by cross-checking it against
`AT_EXECFN`, an auxiliary value the kernel itself provides at process
startup recording the path it actually resolved and executed — a second,
kernel-sourced source of truth for what name a program was really
invoked under, independent of whatever the caller claimed.

== Shells: what actually happens after login

A shell is not special from the kernel's point of view — it's an
ordinary program, distinguished only by what it does: read a line of
text, parse it into a command and its arguments, `fork()` a child
process, `exec()` the named command inside that child, wait for it to
finish, and repeat. Running *interactively* (prompting at a terminal) and
running as a *script interpreter* (reading commands from a file, no
prompt) are the same binary in two different modes, not two different
programs.

`sh` names the POSIX-standardized baseline shell language every Unix
scripting relies on being available; `bash` is a strict superset of it
— arrays, `[[ ]]` conditionals, command history, and more — and is
commonly *also* what `/bin/sh` itself points at, making the extended
dialect available under the POSIX name too (as opposed to systems that
deliberately point `/bin/sh` at a smaller, POSIX-only implementation for
scripts, keeping `bash` reserved for interactive login use). This system
takes the former approach: `/bin/sh` is a plain symlink to `bash`.

== Authentication: what `login` actually checks

Two files, both keyed by username, hold everything `login` needs:

#dtable(
  columns: (auto, 1fr),
  ([File], [What it holds]),
  ([`/etc/passwd`], [One line per account: username, a password placeholder, numeric UID/GID, a display name field, the account's home directory, and its login shell. World-readable by design — plenty of software needs to look up a username or resolve a UID to a home directory.]),
  ([`/etc/shadow`], [The actual password data, split out specifically *because* `/etc/passwd` is world-readable: a salted password hash (or, notably, an empty field), and account-aging data — when the password was last changed, and its minimum/maximum age, warning period, and expiry. Readable only by root.]),
)

`login`'s job is simple to state: read a typed username, look it up in
both files, and — if `/etc/shadow`'s password field for that account is
empty — skip the password prompt entirely and let the login proceed
unauthenticated (a deliberate no-password account, not a bug); otherwise
hash whatever the user just typed using the same algorithm and salt
recorded in that stored hash, and compare. Only on a match does it
finish becoming that user's session: set the process's UID/GID to the
account's, make itself the session leader and attach the controlling
terminal, `chdir` to the account's home directory, and finally `exec()`
the shell named in `/etc/passwd` — replacing itself entirely, not
spawning it as a child. *PAM* (Pluggable Authentication Modules) is the
layer many systems interpose here instead, so login can be extended with
other authentication sources (fingerprint readers, LDAP, two-factor)
without changing `login` itself — not used in this workspace; `login`
here does the passwd/shadow check directly.

== Not every familiar command is coreutils

`grep`, `sed`, `find`, and `ps` feel like they belong on the same list as
`ls`/`cat`/`cp` above — they're just as ubiquitous, just as taken for
granted. They aren't coreutils, though, in GNU's original tool or in
`uutils`' reimplementation of it: GNU grep, GNU sed, GNU findutils, and
procps-ng are four separate, independently-versioned upstream projects,
each with its own release cadence and its own real (if today mostly
overlapping) transitive dependencies — GNU sed, for instance, links
against `libselinux` and `libpcre2` by default via a gnulib module that
probes for SELinux context-preservation support unconditionally, neither
of which has anything to do with basic stream editing.

This distinction is easy to get wrong precisely because it doesn't show
up anywhere obvious: a multi-call binary's symlink (§5.1) can point at
any name at all, including one that isn't a real applet of that binary,
and the failure — `unknown program 'grep'` — only appears the moment
something actually tries to run it, never at build time. This workspace
got exactly that wrong for a while: `uutils`' own applet-symlink list
included `grep`/`sed`/`find`/`ps` as if they were coreutils, installing
dangling symlinks under `/bin` that dispatched to nothing. Fixed by
removing them from that list and building the real, separate projects
instead — GNU grep 3.11, GNU sed 4.9, GNU findutils 4.10.0, and
procps-ng v4.0.7 — each its own small, self-contained autotools build.

= Graphics, Conceptually: From a Kernel Driver to a Frame on Screen

== What a compositor actually does

Every other layer in this section exists to serve one job: a
*compositor* is the one program that owns the display and decides what
actually appears on it. Concretely, it's three responsibilities in one
process:

#dtable(
  columns: (auto, 1fr),
  ([Job], [What that means]),
  ([Window server], [Every other graphical program — a *client*, in Wayland's own terminology — connects to the compositor over a socket and hands it rendered content, rather than drawing to the screen itself. No client ever touches the display directly.]),
  ([Compositing], [With more than one window (or a panel, a cursor, a notification popup) on screen at once, something has to combine all of their separately-rendered content into the single final image actually sent to the display — that combining step is what gives "compositor" its name.]),
  ([Input routing], [Keyboard and pointer/touch events arrive as raw evdev input (§3, via `libinput`, §10.3.3) with no idea which window they're meant for — the compositor decides which client currently has focus and forwards each event there.]),
)

A *window manager*, in the older X11 sense — deciding where windows are
placed, sized, and stacked — is usually just another responsibility the
same compositor process takes on directly in a Wayland system, rather
than a cooperating second program the way X11 traditionally split it.
`cosmic-comp` (§10.3.6) is COSMIC's own compositor; `Weston` (§10.3.5)
is the reference compositor this workspace verified the graphics stack
against first, before `cosmic-comp` existed as a buildpack.

Getting from "a kernel that can talk to a GPU" to "a compositor rendering
a client's window on an actual display" passes through several distinct
layers, each solving a different part of the problem:

#layerstack(
  ("GPU driver (kernel)", "i915, amdgpu, virtio_gpu, … — one per GPU family"),
  ("DRM/KMS core (kernel)", "the generic buffer/mode-setting framework every driver above plugs into"),
  ("libdrm", "ioctl wrapper every layer above builds on"),
  ("Mesa / Gallium", "GL calls → this GPU's own commands"),
  ("EGL / GBM", "context + buffer allocation"),
  ("Compositor", "renders and scans a frame out"),
)

#dtable(
  columns: (auto, 1fr),
  ([Layer], [What it actually is]),
  ([GPU driver (kernel)], [The actual per-hardware driver — `i915` (Intel), `amdgpu`, `virtio_gpu` (for a virtual machine's paravirtualized GPU), and so on. Implements DRM's generic callbacks *for one specific GPU family*; nothing above this layer talks to hardware registers directly.]),
  ([DRM/KMS (kernel)], [The generic subsystem every GPU driver plugs into: hands out and tracks buffers, submits command buffers, configures what's actually scanned out to a display. Has no idea what "draw a triangle" means, and no hardware-specific code of its own — purely a resource-management, submission, and mode-setting *framework*.]),
  ([Mesa], [The userspace library that turns OpenGL/OpenGL ES calls into whatever a specific GPU actually understands.]),
  ([Gallium], [Mesa's own internal plumbing for doing that translation once per GPU *family* rather than once per API. A "Gallium driver" is the translator for one specific GPU — or, for a virtual machine, one specific *virtual* GPU.]),
  ([DRI], [Direct Rendering Infrastructure — the convention by which an application actually finds and loads the right driver at runtime.]),
  ([EGL], [The glue between a window system (Wayland, X11, …) and an OpenGL/GLES context. What a compositor or client actually links against directly — not Mesa's internals.]),
  ([GBM], [Generic Buffer Management — how a Wayland compositor allocates the actual pixel buffers it hands to the DRM/KMS display hardware.]),
)

== DRM and KMS: what the kernel actually owns

*DRM* (Direct Rendering Manager) is a generic *framework*, not hardware
code itself — the actual per-GPU driver (`i915`, `amdgpu`, `virtio_gpu`,
…) is a separate kernel module that implements DRM's callbacks for one
specific GPU family; DRM/KMS core provides the buffer/mode-setting
machinery every one of those drivers plugs into, uniformly, regardless
of which GPU is actually underneath. DRM itself is really two jobs in
one subsystem. The first, *GEM* (Graphics Execution Manager), is buffer-object
management: every chunk of GPU-accessible memory — a texture, a
framebuffer, a command buffer — is a GEM object, referenced by a handle
the kernel hands back to whichever process allocated it, and GEM is what
tracks who owns what and submits queued command buffers to the GPU in
order. The second, *KMS* (Kernel Mode Setting), is display
configuration: enumerating the physical *connectors* (an HDMI port,
say), the *CRTCs* (the hardware pipeline that scans a buffer out to a
connector at a given resolution/refresh rate), and the *planes* a CRTC
can composite together — and letting userspace configure all of it
through one atomic ioctl that either applies a whole new configuration
or fails without touching anything, rather than a sequence of individual
calls that could leave the display in a half-configured state partway
through.

DRM exposes two different kinds of device node for this, deliberately
separated by privilege: `/dev/dri/card0` (the *primary* node) is what
mode-setting and display configuration happens through — only ever
opened by whichever single process currently owns the display, the
compositor. `/dev/dri/renderD128` (a *render* node) is for GPU work that
has nothing to do with display configuration — submitting rendering or
compute work, with no ability to touch what's actually on screen. A
sandboxed or unprivileged client can safely be handed a render node; it
can never be safely handed the primary node too.

== Buffer objects, and sharing them without copying

Not every buffer needs GPU acceleration to produce — a plain, linear,
CPU-writable framebuffer (a *dumb buffer*, in DRM's own terminology,
created via `DRM_IOCTL_MODE_CREATE_DUMB`) is what a kernel framebuffer
console uses, since it just needs somewhere to memcpy text glyphs into,
not a GPU pipeline. Actual rendering — anything Mesa/Gallium produces —
allocates real, driver-specific GEM buffer objects instead, through
`libgbm` (below), sized and laid out however that particular GPU driver
needs them internally.

Getting a buffer from "something Mesa just rendered into" to "something
DRM/KMS scans out to a display" without a wasteful copy is `dma-buf`'s
job: a kernel framework for exporting any GEM buffer as a plain file
descriptor that a *different* subsystem (or process) can import and use
directly — the same underlying memory, not a copy. This is the handoff
that makes zero-copy presentation possible at all: a compositor renders
into a buffer via EGL/Gallium, exports it as a `dma-buf` fd, and hands
that exact fd to KMS to display, with the GPU's own rendered pixels
never round-tripping through the CPU.

== Mesa, Gallium, and finding the right driver at runtime

Mesa doesn't hard-code one driver into `libGL`/`libEGL` — which Gallium
driver actually handles a given GPU is resolved at *runtime*, via *DRI*:
the loader inspects which DRM device is being used (its PCI vendor/device
ID, for real hardware, or its virtual device identity for `virtio-gpu`)
and `dlopen()`s the matching driver shared object out of Mesa's own DRI
driver directory. This is what lets one Mesa install support several
different GPU families simultaneously without an application needing to
know or care which one it's actually running against — including a
Gallium driver for a device that only exists inside a virtual machine.

== EGL and GBM: from an API call to an allocated buffer

*EGL* is what a client or compositor actually calls to get a usable
OpenGL/GLES rendering context tied to a specific window-system surface —
`eglCreateWindowSurface` and friends. It needs a concrete *platform*
implementation to know what a "surface" even means for a given window
system: `platform_wayland` for an ordinary Wayland client, and, for the
compositor itself (which has no window system underneath it — it *is*
the window system), `platform_gbm`. *GBM* is what backs that
platform: `gbm_device` wraps a DRM file descriptor, `gbm_surface`/
`gbm_bo` allocate actual buffers sized and tiled correctly for that
GPU's KMS scanout path, and `gbm_bo_get_fd()` is the call that exports
one of those buffers as the `dma-buf` fd KMS ultimately consumes.

== Getting a rendered frame onto the actual display

The compositor's own per-frame loop, once everything above is in place,
is genuinely simple: render the next frame into a GBM buffer via EGL,
then hand that buffer to KMS as the next scanout buffer for a CRTC/plane
via an atomic commit (conceptually a `drmModePageFlip`, though the
modern atomic API folds this into the same all-or-nothing ioctl KMS
configuration itself uses) — timed to the display's own vertical blank,
not applied instantly mid-scan. Completion isn't polled for: the
compositor reads its own DRM file descriptor as part of its normal event
loop, and a page-flip-complete event arrives on it once the hardware has
actually switched to scanning out the new buffer, which is also the
compositor's cue that the *previous* buffer is now safe to reuse or
free.

== virtio-gpu and virgl: this same stack, virtualized

Everything above assumes a real GPU underneath, but QEMU doesn't
present one when running as a plain virtual machine — `virtio-gpu` is a
*paravirtualized* GPU device instead: the guest kernel's `virtio_gpu`
DRM driver looks, from Mesa's side, like an ordinary DRM device (it
still exposes GEM, KMS, render/primary nodes), but the actual rendering
commands a guest's `virgl` Gallium driver produces are serialized and
sent over the virtio transport to QEMU's own `virglrenderer`, which
replays them against the *host's* real OpenGL driver and GPU. The guest
never touches host GPU memory directly; it gets real GPU-accelerated
rendering by proxy. `softpipe`, by contrast, is a genuine software
rasterizer with no host GPU involved at all — pure CPU — and is what
this workspace's graphics stack (§10) actually falls back to inside
QEMU, `virgl`'s host-side `virglrenderer` support not being assumed
present on every build/test host.

A minimal Wayland-only graphics stack built from source needs, at
minimum: the kernel's DRM driver for the target GPU, `libdrm` (the
kernel-userspace ioctl wrapper every one of these layers builds on), and
Mesa itself providing `libEGL`/`libGLESv2`/`libgbm` plus at least one
Gallium driver. `llvmpipe` (an LLVM-based software rasterizer) is one
possible fallback driver, not a hard requirement — `virgl` and
`softpipe` are two others that work without pulling LLVM into the build
at all.

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
  ([#cd[buildpacks]], [One implementation per upstream package — kernel, coreutils, every library in §9–10's tables — shared between whichever distro(s) actually use each one.]),
  ([#cd[builder-tui]], [A generic interactive terminal dashboard for running any stage of either distro's pipeline and writing the result to a USB stick.]),
  ([#cd[distroless]], [musl + BusyBox + uutils — a small, minimal distro. §9.]),
  ([#cd[distro]], [A from-scratch *glibc* system, native-compiled, aimed eventually at a COSMIC desktop. §10.]),
  ([#cd[distro-init]], [A from-scratch PID 1 written for this project, \~100 lines of Rust — used by `distro` (§10.1.1).]),
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
  ([uutils/coreutils], [`ls`, `cat`, `cp`, …], [The musl variant of the same `Uutils` buildpack `distro` uses (§10.3.1) — one multi-call binary, statically linked, musl-static by default.]),
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
exactly the "purpose-written init" pattern described in §2.5.

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
`distro build-kernel` fetches, configures, and compiles it.

=== Kconfig and `.config`: how kernel configuration actually works

Every buildable kernel feature — a driver, a subsystem, an optional
instrumentation hook — is declared in *Kconfig*, a dependency-aware
description language: an option can be built (`y`, directly into the
kernel image), built as a separate loadable module (`m`), or left out
entirely (`n`), and Kconfig itself enforces that an option can't be
turned on if whatever it depends on is off. `.config` is the flat,
resolved output of that — one `CONFIG_FOO=y`/`m`/`n` line per option —
and it's `.config`, not the Kconfig source tree, that `make` actually
reads to decide what to compile.

`make defconfig` produces a *curated* starting `.config` — each
architecture ships its own reasonable, working baseline, not "every
option off" — which is why stripping this project's own optional
feature packs back out (below) starts from a real, bootable
configuration rather than building one up from nothing. `make
olddefconfig` is the companion operation for the opposite situation:
given an existing `.config` (possibly written by a different Kconfig
tree version entirely — a newer kernel adds options an older `.config`
never had opinions about), it fills in every option the current tree
knows about but the file doesn't mention with that option's own
default, non-interactively. This project uses it in both of §10.2's
starting points: after `defconfig`'s own baseline gets edited by pack
stripping, and to re-resolve a previously saved, hand-picked config that
may predate a kernel version upgrade.

=== Built-in vs. module: why nothing here needs loading

An option built as `y` is simply part of `vmlinuz` — present the moment
the kernel starts, nothing further to do. An option built as `m` instead
produces a separate `.ko` file, meant to be found under
`/lib/modules/<version>/` and explicitly loaded (by an early-boot module
loader, or a later `modprobe`) before whatever it provides becomes
available — a real mechanism, and the right choice for a general-purpose
distribution that can't predict every piece of hardware it'll run on
ahead of time. This distro's own kernel takes the other path: every
feature it builds is `y`, never `m` — no `/lib/modules` install step
exists in its own pipeline (§10.5) at all — which is exactly what makes
skipping an initramfs (§2.3) safe: there's no module-loading step this
kernel could ever be waiting on before it can mount its own root
filesystem.

=== Feature packs, and getting from a stripped baseline to the final config

The starting config is either `make defconfig` (the default) with every
optional *feature pack* below stripped back off, or a previously saved
`.config` from an interactive `make menuconfig` session
(`distro menu-config`), re-resolved via `make olddefconfig`. Either way,
whatever packs are named in `kernel.features` get turned back on as the
final step.

#flow("defconfig or saved .config", "strip feature packs", "re-enable kernel.features", "olddefconfig", "final .config")

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

`distro.toml` currently sets `kernel.features = ["graphics"]` — the one
pack the graphics stack (§6, §10.3.4) actually needs — with every other
pack left off the fully stripped `defconfig` baseline.

== Packages

`distro` builds a separate, purpose-specific package for each job a
desktop-capable system needs done — laid out here in dependency order,
bottom of the stack first: the static base first (needs nothing else
already built), then the seat/session layer, then the Wayland-core
libraries, then the graphics stack proper, then everything Weston itself
needs.

=== The static base

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

=== Seat & session daemons

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

=== The Wayland-core libraries

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

=== The graphics stack

From §6's conceptual layering down to what's actually built here — the
one place in this whole table where a *kernel-side* driver matters as
much as anything built from userspace source, since nothing above it
works without the right one already compiled in via `kernel.features`
(§10.2.3):

#layerstack(
  ("virtio_gpu (kernel driver)", "enabled via the graphics feature pack, §10.2"),
  ("DRM/KMS core (kernel)", "generic framework virtio_gpu plugs into"),
  ("libdrm", "2.4.134 — generic core only, no vendor sub-libraries"),
  ("Mesa / Gallium", "virgl + softpipe Gallium drivers, no LLVM"),
  ("Weston", "the compositor actually hosting a client"),
)

#dtable(
  columns: (auto, auto, 1fr),
  align: (left, left, left),
  ([Package], [Version], [What it's for]),
  ([libdrm], [2.4.134], [The kernel-userspace ioctl wrapper every GPU-facing library builds on — every vendor-specific sub-library disabled, virtio-gpu needs only the generic core.]),
  ([Mesa], [26.2.2], [`libEGL`, `libGLESv2`, `libgbm`, and the Gallium driver — built scoped to `virgl` (talks to QEMU's virtio-gpu/virgl backend) and `softpipe` (software fallback), plus `lavapipe` (the software Vulkan ICD, §10.3.6.3), statically linked against LLVM so the target image carries no runtime `libLLVM.so` dependency. GLX/X11 stays disabled — Wayland/EGL/GLES/Vulkan only.]),
)

The kernel driver itself — `virtio_gpu`, part of the `graphics` feature
pack (§10.2.3, `CONFIG_DRM_VIRTIO_GPU`) — is not a separate buildpack:
it's compiled directly into `vmlinuz` alongside the rest of the kernel,
the same `y`-not-`m` choice §10.2.2 covers for every other driver this
kernel needs. Nothing in `libdrm`/Mesa's own build depends on which GPU
driver ends up underneath at runtime — that binding happens at boot,
when the kernel probes for a matching device and the driver it finds
(virtio-gpu, here, since that's what QEMU presents) is whatever DRI then
loads a Gallium driver against (§6.4).

=== Hosting a compositor: the Weston chain

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

=== cosmic-comp: COSMIC's own compositor

COSMIC (`pop-os/cosmic-epoch`) is 28 real components (§11); `cosmic-comp`
— the compositor itself, built on `smithay` — is the first one, and
everything else in a minimal session depends on it existing first. It
needs no C library buildpacks beyond what Weston's own chain already
built: `smithay`'s feature set links against `libseat`, `libinput`,
`libdrm`+`gbm`, `libudev`, and `libxkbcommon`, all of which this sysroot
already provides — D-Bus access goes through `zbus`, a pure-Rust
implementation of the wire protocol, not a `libdbus` binding, so it adds
no C dependency either.

One real upstream issue, found and patched: `cosmic-comp`'s own
`Cargo.toml` names a `[patch]` replacement source with a doubled slash
(`https://github.com/pop-os//cosmic-protocols`) — a typo that happens to
matter, not a no-op, since Cargo's git-source deduplication treats it as
a *different* string from the correctly-spelled main dependency, letting
the patch silently win. Fixing the typo makes Cargo correctly reject the
patch as redundant instead (same source, two different refs) — so the
buildpack's patch fixes the typo *and* removes the now-redundant `[patch]`
block, retargeting the main dependency to the `branch = "main"` ref the
patch (and the project's own committed `Cargo.lock`) already resolved to.

#callout(kind: "ok", "Verified")[
  A full `cargo build --release` succeeds against this workspace's real
  sysroot (no new C library buildpacks needed), and `readelf -d` on the
  resulting binary shows ten direct `NEEDED` entries — `libdisplay-info`,
  `libgbm`, `libseat`, `libudev`, `libinput`, `libpixman-1`,
  `libxkbcommon`, `libgcc_s`, `libm`, `libc` — every one of which
  resolves cleanly against the sysroot with zero "not found" entries.
  `distro build-userland`'s real topological sort places it correctly
  among its dependencies, and `assemble-rootfs` installs it to
  `/usr/bin/cosmic-comp`. It actually runs, too — see the `distro-init`
  service below.
]

==== Actually running it: a `distro-init` service

Building `cosmic-comp` and having something start it are two different
milestones — Weston's own real-boot moment above needed the same
distinction, and was started by hand from a login shell rather than
automatically. `cosmic-comp` instead gets a real `distro-init` service,
supervised the same way as `seatd`/`dbus-daemon`/`udevd`: forked, execed,
and respawned if it ever exits.

#flow("udevd/seatd/dbus/gettys start", "udevadm settle (bounded, 10s)", "mkdir + chmod 0700 XDG_RUNTIME_DIR", "fork+exec cosmic-comp", "supervised — respawn on exit")

Two things distro-init has to get right that the other four services
don't need:

- *`XDG_RUNTIME_DIR` must exist first.* `wayland-server` refuses to
  create its socket without it, and refuses a directory with the wrong
  permissions — it has to be `0700`, owned by whoever's running the
  compositor. `distro-init` creates `/run/user/0` (root, since nothing
  in this image drops privileges yet) with exactly that mode before
  ever forking `cosmic-comp`.
- *Devices need to have actually appeared first.* `agetty`/`seatd`/
  `dbus-daemon`/`udevd` itself don't touch `/dev/dri` or `/dev/input` at
  their own startup, so they can all start immediately, racing udevd's
  coldplug queue with no ill effect. `cosmic-comp`'s DRM/libinput
  backend does — probing for a GPU it needs to have a device node for.
  `distro-init` runs `udevadm settle --timeout=10` (bounded, so a stuck
  queue can't hang boot forever) right before spawning it, the one place
  in this whole init sequence something actually waits on udev.

No VT-attachment dance was needed: `seatd`'s VT-bound seat (§4) assigns
a connecting client whatever VT the kernel currently reports as
*active* (`seat_add_client` in `seatd/seat.c`, read via an ioctl on
`tty0`), not whatever terminal the requesting process happens to be
attached to. Since nothing has switched consoles by the time
`distro-init` forks `cosmic-comp`, that's VT1 — the same VT `agetty`'s
own tty1 login prompt lives on, with no conflict, since `agetty` was
never a `libseat` client to begin with.

#callout(kind: "trap", "A real bug this surfaced: libtinfo.so.6")[
  The first boot attempt crashed on start:
  `Failed to load LibEGL: DlOpen { desc: "libtinfo.so.6: cannot open
  shared object file" }`. Mesa's `lavapipe` Vulkan ICD (§10.3.6.3)
  statically links the host's own LLVM, and that host LLVM build was
  itself linked against `libtinfo` (terminal color-support detection) —
  a genuinely new runtime dependency that `-Dllvm=enabled` introduced
  but that nothing had actually exercised until `cosmic-comp` became the
  first thing to `dlopen()` the DRI/EGL loading chain at runtime. Fixed
  the same way this project already handles `libgcc_s`/`libstdc++`/
  `libz`/`libzstd`/`libffi` (`distro/src/stages/rootfs.rs`'s
  `HOST_DYNAMIC_LIBS`, §10.4) — `libtinfo.so.6` copied straight from the
  host, since we compile natively against the host's own glibc anyway.
]

#callout(kind: "ok", "Verified — real boot, no crash loop")[
  A scripted QEMU boot: `cosmic-comp` starts after the settle above,
  connects to `seatd` (`seat.c` logs `Opened client 1 on seat0`),
  initializes its DRM/KMS backend and EGL context, and opens a real
  Wayland socket at `$XDG_RUNTIME_DIR/wayland-1`. Confirmed still
  running and un-crashed ~40 seconds later by checking `/proc/<pid>/comm`
  from a logged-in shell, with zero `distro-init: cosmic-comp exited,
  respawning` messages anywhere in the boot log — the failure mode that
  would show up immediately if it were crash-looping.
]

#callout(kind: "trap", "At this point: no client had ever connected")[
  Running and un-crashed is not the same as rendering something anyone
  would see. This boot's log shows several non-fatal warnings —
  Xwayland failing to start (no X server exists in this image, matching
  the Rust/C-only rule — see the callout at the end of §11), and both
  the session and system D-Bus connections failing (no session bus is
  running, and `cosmic-comp`'s system-bus lookup path doesn't match how
  this image's `dbus-daemon` was started) — none of which stop the
  compositor itself from coming up, but none of which are actually
  fixed either. What's closed below (§10.3.6.2) is the more important
  gap this callout used to describe: nothing had connected to
  `wayland-1` and asked it to render a frame — the same "not yet a
  visible pixel" gap Weston closed with `weston-simple-egl`.
]

==== cosmic-bg: the first real client

`cosmic-bg` — COSMIC's background/wallpaper renderer — is the second
real COSMIC component built here, and the first thing to actually
connect to `cosmic-comp`'s socket and render something. It's a
standalone Wayland client (a `wlr-layer-shell` background surface via
`smithay-client-toolkit`), not part of `cosmic-session`'s own launch
chain, so — like `cosmic-comp` before it — it needs no session
orchestrator to exist first, just a compositor to connect to.

#flow("cosmic-comp opens wayland-1", "cosmic-bg connects (WAYLAND_DISPLAY=wayland-1)", "layer-shell background surface", "solid color rendered")

Two real gaps, found and fixed the same way every other gap in this
report was — by actually trying to boot it, not by predicting them:

#callout(kind: "trap", "Bug: wayland-0 doesn't exist, on purpose")[
  `cosmic-bg` crash-looped on every boot with `Could not find wayland
  compositor` — not a startup race, a hard failure, every single time.
  The cause: smithay's `ListeningSocketSource::new_auto()` (what
  `cosmic-comp` uses to open its socket, §10.3.6.1) deliberately skips
  `wayland-0` — its own source comment reads "we don't try wayland-0
  since clients may connect to the wrong compositor" — and starts
  numbering at `wayland-1` instead. `cosmic-bg` left `WAYLAND_DISPLAY`
  unset, which defaults to `wayland-0`, a socket that will never exist
  in this image. Fixed by having `distro-init` set
  `WAYLAND_DISPLAY=wayland-1` explicitly when spawning it — safe to
  hardcode since `cosmic-comp` is the only Wayland server this image
  ever runs, so nothing else could claim that name first.
]

#callout(kind: "trap", "Bug: the default wallpaper doesn't exist either")[
  `cosmic-bg-config`'s fallback entry (used whenever no `cosmic-config`
  state exists — every fresh boot of this image) points at
  `/usr/share/backgrounds/cosmic/orion_nebula_nasa_heic0601a.jpg`, part
  of the separate `cosmic-backgrounds` data package this project
  doesn't build or ship. Left alone, `cosmic-bg` would find no file
  there and quietly render nothing — a whole extra running service for
  zero visible effect. The buildpack's one source patch changes the
  fallback from `Source::Path` to `Source::Color`, a plain solid color,
  so a fresh boot always has something real to show without needing any
  wallpaper image asset at all — arguably a better default for a kiosk
  than a stock nebula photo anyway.
]

#callout(kind: "ok", "Verified")[
  A scripted QEMU boot: after one expected respawn (racing
  `cosmic-comp`'s own socket creation, same self-healing pattern
  `distro-init` already uses everywhere), `cosmic-bg` connects and stays
  up — confirmed alongside `cosmic-comp` via `/proc/<pid>/comm`, with no
  further `distro-init: cosmic-bg exited` messages afterward.
  `readelf -d` shows only `libgcc_s`/`libm`/`libc` as `NEEDED` — even
  smaller than `cosmic-comp`'s own footprint, since `wayland-client`
  here is the pure-Rust `wayland-backend` crate talking directly to the
  socket, no `libwayland-client.so` involved. Not independently
  re-confirmed visually in *this* session (no screenshot taken) — a
  QEMU window with just a compositor-drawn cursor and no client was
  seen earlier in this same line of work, before `cosmic-bg` existed;
  whether the solid color actually appears on screen now is expected,
  not yet re-checked.
]

==== A working Vulkan stack: driver, headers, and loader

Getting Vulkan from "compiles" to "usable at runtime" needed three
separate pieces, not one:

#layerstack(
  ("cosmic-comp (dlopen's libvulkan.so.1 at runtime)", "no build-time link — smithay's ash crate needs no Vulkan SDK"),
  ("Vulkan Loader (libvulkan.so.1)", "KhronosGroup/Vulkan-Loader, CMake — reads /usr/share/vulkan/icd.d/*.json"),
  ("lavapipe ICD (libvulkan_lvp.so)", "Mesa, built statically against LLVM"),
  ("Vulkan Headers", "KhronosGroup/Vulkan-Headers — headers + pkg-config/CMake package files only, no library"),
)

Mesa's own Vulkan driver (`lavapipe`, enabled via
`-Dvulkan-drivers=swrast`, §10.3.4's table) is an *ICD* (Installable
Client Driver) — the actual implementation an application talks to once
it's found. It is not, by itself, enough: applications don't link
against an ICD directly, they `dlopen()` the Vulkan *loader*
(`libvulkan.so.1`, a separate KhronosGroup project from Mesa), which
then reads `/usr/share/vulkan/icd.d/*.json` manifests at runtime to find
and dispatch to whichever ICD is installed. Without the loader, Mesa's
ICD sits on disk unreachable — nothing in this workspace had ever built
one before this pass.

Two new buildpacks close that gap: `vulkan_headers` (headers-only,
`InstallMode::Sysroot`, needed at build time by both Mesa and the loader
itself) and `vulkan_loader` (depends on `vulkan_headers`; built with
`-DBUILD_WSI_WAYLAND_SUPPORT=ON` and XCB/Xlib support both off, matching
this workspace's Wayland-only policy elsewhere). Both are CMake-based —
the first CMake support this project needed, everything before them
having been autotools or meson.

#callout(kind: "ok", "Verified")[
  `readelf -d` on the built `libvulkan.so.1` shows exactly one `NEEDED`
  entry, `libc.so.6` — the loader itself has no other link-time
  dependencies, by design, since it finds ICDs by `dlopen()` at runtime
  rather than linking them. `/usr/share/vulkan/icd.d/lvp_icd.x86_64.json`
  (installed by the Mesa buildpack) correctly names
  `/usr/lib/x86_64-linux-gnu/libvulkan_lvp.so` as its ICD's
  `library_path`. `distro build-userland`'s topological sort places both
  new packages correctly (`vulkan_headers` before `vulkan_loader`, both
  before `mesa`), `assemble-rootfs` installs `libvulkan.so.1` and the ICD
  manifest into the rootfs, and a full `make-image` plus QEMU boot
  (§10.3.5's callout) still reaches a clean login prompt with both in
  place.
]

#callout(kind: "trap", "Still not confirmed: an application actually using it")[
  The loader finding the ICD manifest is not the same as `cosmic-comp`
  successfully creating a `VkInstance` and rendering a frame through it.
  `cosmic-comp` does now actually run, as a real `distro-init` service
  (§10.3.6.1 above) — but `smithay`'s renderer selection there picked
  its GL/EGL path, not Vulkan (nothing in this boot's log mentions
  `VkInstance` creation), and no client has connected to ask it to
  render anything through either path yet (§10.3.6.1's last callout).
  Deliberately, `vulkan_loader` was *not* added to `cosmic_comp`'s own
  `dependencies()` list: this project's `dependencies()` edges are
  build-order only, and `cosmic-comp` never links against the loader at
  build time, only `dlopen()`s it at runtime — so nothing in the
  dependency graph would catch a broken runtime lookup. Confirming
  Vulkan actually gets exercised end-to-end is future work.
]

==== Real GPU acceleration: virtio-gpu-gl, not the bochs stub

Every QEMU boot this whole project has ever run — from the first
Weston milestone through every `cosmic-comp`/`cosmic-bg` test above —
actually used QEMU's implicit default display device: a scanout-only
"bochs" VGA stub (PCI id `1234:1111`), never `virtio-gpu`, despite the
kernel's own `graphics` feature pack enabling `CONFIG_DRM_VIRTIO_GPU`.
`test_qemu`'s own `qemu-system-x86_64` invocation never asked for a
better one, and the doc's own earlier claims about "virtio-gpu/virgl"
were aspirational, not verified — nothing before this checked what
device was actually present.

This went unnoticed because nothing here had ever needed a real GPU
render node: Weston fell back to its `softpipe` Gallium driver,
`cosmic-bg` only ever used `wl_shm` (plain CPU-written pixel buffers,
§10.3.6.2), and `cosmic-comp`'s own compositing worked via KMS
dumb-buffer scanout — none of that needs actual 3D/render capability.

#callout(kind: "trap", "What this broke: any client needing real EGL/GL")[
  A GPU-accelerated Wayland client (the first attempted here was
  Alacritty, §10.3.6.5) got nothing to render through. Confirmed with
  `WAYLAND_DEBUG=1`: `cosmic-comp` advertised 57 Wayland globals on
  connect, and neither `zwp_linux_dmabuf_v1` nor `wl_drm` — the *only*
  two protocols a client can use to discover which DRM device to hand
  EGL — was among them, because the bochs stub has no render node to
  advertise in the first place. Mesa's own `glGetDriverName(fd -1)`
  call then failed exactly as its error message says.
]

Fixed in `buildpack_core::pipeline::TestQemu`: `-vga none -device
virtio-gpu-gl-pci` (suppressing the implicit bochs default), plus a
matching display backend — `-display gtk,gl=on` for `--window` mode,
`-display egl-headless` for headless/scripted testing (replacing
`-nographic`, which forces `-display none`, incompatible with any GL
backend).

#callout(kind: "ok", "Verified — real GPU acceleration, end to end")[
  The kernel now reports `[drm] pci: virtio-gpu-pci detected`,
  `features: +virgl`, `Initialized virtio_gpu 0.1.0`, and `/dev/dri`
  has a genuine `renderD128` alongside `card0` — none of which ever
  appeared before. `weston-simple-egl` (§10.3.5's own milestone,
  bundled with Weston's source but a generic client with no dependency
  on Weston-the-compositor — anything on `WAYLAND_DISPLAY` will do)
  run against `cosmic-comp`'s socket renders a continuously-spinning
  textured triangle, proving the *whole* chain — virtio-gpu → virgl →
  host GPU → EGL → `cosmic-comp`'s own compositing → a real frame —
  works correctly end to end. The standard plain-boot regression check
  (login prompt, `cosmic-comp`/`cosmic-bg` both still starting)
  confirmed this didn't break anything already working.
]

==== Alacritty and DejaVu: a real terminal, mostly

`cosmic-term` (§11) is a full `libcosmic`/`iced` application — a much
heavier dependency chain than anything needed just to prove a terminal
can run here. Alacritty was picked instead: plain `winit`+`glutin`, no
COSMIC-specific dependencies at all, and genuinely lighter than
`cosmic-term` would be. Ghostty was considered and rejected outright —
its core is Zig, not Rust or C, the standing rule from §11's closing
callout.

Getting it running surfaced two more real gaps, in order:

#callout(kind: "trap", "Gap 1: no font files, anywhere")[
  `fontconfig` (the library) has been built and configured since the
  Weston chain (§10.3.4), but nothing had ever shipped an actual font
  *file* for it to resolve `"monospace"` to — Alacritty failed outright
  with `Font(FontNotFound(...))`. Fixed with a new `dejavu_fonts`
  buildpack: pure data (pre-built TrueType files + fontconfig alias
  snippets), the same category as `xkeyboard_config`'s XML/lua data —
  DejaVu is only distributed upstream as release tarballs, not
  buildable from source without FontForge (a GUI font editor), so this
  isn't a Rust/C-only rule concern any more than a keyboard layout
  table is. Installs to `/usr/share/fonts/dejavu` and
  `/etc/fonts/conf.d`, both already scanned/loaded by this sysroot's
  own `fonts.conf`. Fixing this also surfaced that `/var/cache/fontconfig`
  (fontconfig's own declared cache directory) and `/tmp` had never
  existed in this rootfs at all — harmless until something actually
  needed them, now both created by `assemble-rootfs`.
]

#callout(kind: "trap", "Gap 2 (resolved): no /dev/pts, not winit at all")[
  With real fonts and real GPU acceleration both in place, Alacritty
  got all the way through EGL context creation, font loading, and
  window/PTY setup — confirmed via `-vvv` logging, including `Running
  on virgl (Mesa Intel(R) Iris(R) Xe Graphics ...)`, the host's own
  real GPU name flowing all the way through — then exited almost
  instantly afterward (`Goodbye` logged well under a second after `PTY
  dimensions`), with no frame ever rendered and no error beyond a
  bare, contextless `Os { code: 2, kind: NotFound }`. Initially
  suspected as a `winit`-specific Wayland event-loop bug (client-side
  decoration or fractional-scale negotiation), since `weston-simple-egl`
  proved the GPU/compositor stack itself was sound.

  Root-caused later, while getting `sway`+`foot` (§10.3.6.7) working
  against the same compositor: `foot` hit the exact same `Os { code: 2 }`
  class of error, but with an unambiguous message —
  `failed to open PTY: No such file or directory`. `/dev/ptmx` existed
  (a devtmpfs device node), but this rootfs never mounted a `devpts`
  filesystem at `/dev/pts`, so `grantpt()`/`ptsname()` had nowhere to
  resolve the PTY slave — the exact same code path `alacritty_terminal`
  uses. Fixed by mounting `devpts` at `/dev/pts` in `distro-init`
  (alongside the `/dev/shm` fix below). Not `winit`-specific at all:
  re-tested after the fix and Alacritty now runs stably, past PTY setup,
  actively receiving terminal I/O from its shell.
]

==== cosmic-term: a different crash, same eventual fix

With Alacritty's `winit` bug parked (§10.3.6.5's Gap 2), `cosmic-term`
itself was tried as a way to test a specific hypothesis: it depends on a
pop-OS fork of `winit` (`github.com/pop-os/winit`, tag `cosmic-0.14`),
not upstream `winit` — plausibly a way to sidestep Alacritty's bug
entirely, if the bug is upstream-`winit`-specific. Verified via a real
`cargo check --release` against this sysroot before writing the
buildpack: every dependency already resolved, no new C libraries needed
(same `libxkbcommon`/glibc footprint as `cosmic-comp`/`cosmic-bg`, per
`ldd`). One build-time issue, unrelated to the runtime question: unlike
`cosmic-comp`/`cosmic-bg`, `cosmic-term`'s own `Cargo.toml` has no
`[workspace]` table, so cargo's upward search found this project's own
workspace instead — fixed with a `SourcePatch` appending an empty
`[workspace]` table, exactly the remedy cargo's own error suggests.

#callout(kind: "trap", "(resolved) The panic was the missing /dev/pts too")[
  Launched against a live `cosmic-comp` session (`WAYLAND_DISPLAY=wayland-1`,
  the same socket `weston-simple-egl` proved works), `cosmic-term` didn't
  exit quietly like Alacritty — it panicked outright:
  #raw("async fn` resumed after completion") at `iced/winit/src/lib.rs:765` (inside `libcosmic`'s
  own `iced` fork's event-loop glue), the classic symptom of a future
  being polled again after it already returned `Poll::Ready`. Also logged
  (non-fatal, before the panic): repeated `xkbcommon` errors about a
  missing Compose file for the `en_US.UTF-8`/`C` locales — this sysroot
  still has no locale data installed at all, a real but separate,
  cosmetic gap, not pursued.

  Re-tested after §10.3.6.7's `/dev/pts` fix (added for `foot`, and what
  turned out to actually fix Alacritty too, §10.3.6.5): `cosmic-term` no
  longer panics at all. It backgrounds itself via its own `fork`
  dependency (unlike Alacritty, it daemonizes — the parent shell job
  shows `Done`, exit 0, while a detached child keeps running), and stays
  alive indefinitely: confirmed live via `/proc/<pid>/fd` showing open
  `/dev/dri/renderD128` (×3), a real `/dev/ptmx`, Wayland sockets, and
  `smithay-client-toolkit`'s `memfd` GPU buffer allocations. The panic
  was never really `iced`'s event loop misbehaving on its own — almost
  certainly the same PTY-open failure Alacritty and `foot` both hit,
  just surfaced as a mishandled future inside `iced`'s async executor
  instead of a clean `ENOENT`. All three GUI clients this project has
  tried — Alacritty, `foot`, `cosmic-term` — trace back to the single
  `/dev/pts` gap.
]

==== sway + foot: a working GUI terminal, finally

With COSMIC's own stack a dead end for a working terminal (Alacritty's
Gap 2 above, cosmic-term's panic), `sway` — a mature, wlroots-based
tiling WM, C throughout — was tried as a genuinely different compositor
stack, paired with `foot`, a lightweight wlroots-native terminal that
deliberately avoids pango/glib in favor of its own `fcft` font library.

`wlroots` itself needed *zero* new C libraries: every dependency it
probes for — EGL/GBM/GLESv2 (Mesa), libseat (`seatd`), libdisplay-info,
libudev, libdrm, xkbcommon, pixman, wayland — was already in this
sysroot from the Weston/COSMIC/Vulkan work. `sway`'s own `meson.build`
does pull in a real new chain, though: it declares `pango`/`pangocairo`
as unconditional dependencies (not gated by the `swaybar`/`swaynag`
options), which in turn need `glib`+`harfbuzz`+`fribidi` — plus
`json-c`/`pcre2` for sway's own IPC and config-regex parsing. `foot`, by
contrast, only needed its own tiny `fcft`/`tllist` font stack — no glib,
no pango, confirming it's the lighter of the two terminal paths this
project has tried.

#callout(kind: "trap", "hwdata's pkgdatadir gets sysroot-mangled too")[
  wlroots' DRM backend reads hwdata's `pnp.ids` (vendor-name table) at
  build time via a `native: true` pkg-config dependency — but this
  workspace's `PKG_CONFIG_SYSROOT_DIR` (needed so every *other*
  pkg-config lookup here finds the shared sysroot, not the host) still
  mangles hwdata's own `pkgdatadir` variable with that same sysroot
  prefix, since meson's native and host pkg-config are the same single
  invocation in a non-cross build — same bug class as weston's pango
  probe and Mesa's `llvm-config` (§10.3.4/§10.3.6.1). Fixed by staging a
  copy of the host's `pnp.ids` at the exact path the mangled lookup
  resolves to, rather than fighting `sysroot_env`'s global
  `PKG_CONFIG_SYSROOT_DIR` for one native-only dependency.
]

Boot-tested nested inside a live `cosmic-comp` session (same
`WAYLAND_DISPLAY=wayland-1` trick `weston-simple-egl` used), sway
surfaced two more real, previously-hidden rootfs gaps:

#callout(kind: "trap", "No /dev/shm: wlroots' shm allocation failed outright")[
  `wlroots`' `wl_shm`/dmabuf-feedback format-table allocation needs a
  real POSIX shm backing (`shm_open`), and failed immediately —
  `[wlr] [types/wlr_linux_dmabuf_v1.c:537] Failed to allocate shm file
  for format table`, `sway/server.c:292] Failed to create linux-dmabuf
  v1`. `devtmpfs` (already mounted at `/dev`) provides device nodes but
  not a real tmpfs instance for shm; `cosmic-comp`/smithay never hit
  this gap because it prefers `memfd_create` over `shm_open`. Fixed by
  mounting a `tmpfs` at `/dev/shm` in `distro-init`, alongside the
  existing `/proc`/`/sys`/`/dev`/`/run` mounts.
]

#callout(kind: "trap", "No /dev/pts either — and this was Alacritty's bug all along")[
  With `/dev/shm` fixed, sway itself ran and rendered correctly
  (confirmed live via `swaymsg -t get_outputs`: a real 852×496 nested
  output, `swaybar` actively committing surfaces). Launching `foot`
  under it hit a second, separate gap, this time with an unambiguous
  message: `err: terminal.c:1195: failed to open PTY: No such file or
  directory`. `/dev/ptmx` existed (a `devtmpfs` device node), but this
  rootfs never mounted a `devpts` filesystem at `/dev/pts`, so
  `grantpt()`/`ptsname()` had nowhere to resolve the PTY slave. Fixed
  the same way — `mount(devpts, /dev/pts, ...)` in `distro-init`.

  This is the *exact* code path `alacritty_terminal` uses too (both it
  and `foot` open a PTY for their shell the same way), and Alacritty's
  own failure was `Os { code: 2, kind: NotFound }` — `ENOENT`, the same
  class of error, just without `foot`'s clearer message pointing at the
  actual missing path. Re-tested Alacritty against the fixed image:
  it now runs stably, well past `PTY dimensions` (previously the last
  line logged before an instant `Goodbye`), actively receiving terminal
  I/O from its shell. Not a `winit`-specific bug at all — see §10.3.6.5's
  Gap 2, now resolved and cross-referenced here.
]

With both fixes in place, `sway`+`foot` and Alacritty both work: the
first fully working GUI terminals in this from-scratch image, two
independent proofs of the same underlying `/dev/pts` fix.

==== Reaching a session without racing the compositor for the window

Every one of the tests above still needed logging in through the
serial console (ttyS0) and manually exporting `XDG_RUNTIME_DIR`/
`WAYLAND_DISPLAY` before launching anything — a real usability gap for
the actual window `test-qemu --window` opens, not just a scripting
convenience. Closing it took three attempts, each surfacing a different
real bug.

#callout(kind: "trap", "Attempt 1: the login prompt races cosmic-comp for the display")[
  A `/root/.bash_profile` (sourced by `login`'s own exec of a login
  shell, §5.2) is the natural place to set both session variables
  automatically and, specifically on `tty1` — the one console QEMU's
  window actually renders — wait for `cosmic-comp`'s Wayland socket and
  launch `cosmic-term`. Detecting "am I on tty1" needed care: `$(tty)`
  looked obvious but doesn't work here (§5.4's own lesson, a different
  angle on it — `tty` is a real `uutils` applet with no symlink in
  `COREUTILS_APPLETS` at all), so the actual check uses bash's builtin
  `-ef` file-identity test (`[ /proc/self/fd/0 -ef /dev/tty1 ]`) instead,
  paired with a plain counted `while` loop rather than external `seq`.

  None of that mattered on the first real test: `cosmic-comp` takes
  DRM/KMS ownership away from tty1's own text console within a few
  seconds of boot (§10.3.6.1), which in practice is too fast a window to
  reliably see a login prompt there at all, let alone type a username
  into it. The profile script was correct and never got the chance to
  run.
]

#callout(kind: "trap", "Attempt 2: autologin fixes the race, then causes a respawn storm")[
  The fix for the race removes the human-typing step it depended on
  entirely: `agetty --autologin root` on tty1 (only — `ttyS0` stays a
  normal interactive login, still the plain debug-shell path). Root's
  password is already empty (§5.3), so this doesn't weaken anything
  real.

  That surfaced a second, unrelated bug immediately: `cosmic-term`
  daemonizes itself (a real `fork` dependency, unlike Alacritty), so its
  own top-level process always exits almost immediately once it forks
  off the real, detached worker. The profile's `exec cosmic-term` meant
  that exit took the whole `agetty`→`login`→`bash` chain down with it —
  no fork in that chain ever created a process to exit independently of
  the others — which `distro-init` dutifully respawned, autologin and
  all, launching another `cosmic-term`, forever. Five-plus stacked
  `cosmic-term` processes confirmed within 8 seconds of boot.

  Fixed by dropping the `exec`: running `cosmic-term` plain (no `exec`,
  no `&`) lets its own daemonizing exit return control to *this* script
  instead of unwinding the login chain, so it can `break` out of the
  wait loop and fall through to an ordinary, harmless idle shell.
  Verified booting headless with autologin enabled: exactly one
  `cosmic-term` process, stable over 30+ seconds, no respawn loop.
]

Separately, the window itself defaulted to a cramped 1280×800 —
`virtio-gpu-gl-pci`'s own default `xres`/`yres`, not something
`cosmic-comp` or `sway` chooses. Set explicitly to 1920×1080 in
`test_qemu`'s own device args; still just a starting mode, the window
can be resized live afterward and virtio-gpu renegotiates with the
guest same as before.

== The sysroot: how these packages find each other

Static-base packages (§10.3.1) never need each other at
build time — each just needs the host's gcc. Everything from the seat
layer onward does: `wayland-protocols` needs `wayland-scanner` on `PATH`
at its own build time, and `libinput` needs `eudev`'s installed
`libudev.pc` to link against `libudev` at all. Solving that — letting
one from-source package's build see another already-built one, using
the exact same mechanism real software expects a real system install to
provide — is what the *sysroot* actually is.

#flow("Package A: --prefix=/usr", "make install DESTDIR=sysroot", ".pc file in sysroot", "Package B's pkg-config lookup", "Package B build")

=== DESTDIR: staged installation, not a different install

`./configure --prefix=/usr` (or meson's `--prefix=/usr`) doesn't just
control where `make install` copies files to — it's compiled directly
into the resulting binaries, since plenty of software looks up its own
data files, plugins, or config at *runtime* relative to whatever prefix
it believes it lives under. Actually installing to `/usr` while building
would mean every package installs on top of the host's own real
`/usr` — exactly what this project's whole premise rules out. `DESTDIR`
is the standard convention (every autotools/meson project supports it,
because real distribution packaging depends on it) for breaking that
link: `make install DESTDIR=<sysroot>` still installs *as if* the
package's prefix were `/usr`, but physically writes every file under
`<sysroot>/usr/...` instead — the binary's own compiled-in idea of its
prefix stays the real `/usr`, only the location `make install` actually
wrote to changes. This is precisely how `.deb`/`.rpm` build pipelines
stage a package's files before archiving them, reused here for the same
reason: to get a package's files onto disk without overwriting the host.

=== pkg-config: how one package's build finds another's

`pkg-config` itself is not one of this distro's own from-source
packages — like `gcc`, `meson`, `ninja`, and `gperf`, it's a host build
tool `distro build-toolchain` installs via `apt-get`
(`distro/src/stages/toolchain.rs`), the same "host compiler toolchain is
infrastructure, not distro content" exception every from-scratch distro
makes for the tools that do the actual compiling. What follows is how
the packages *it operates on* — the ones this distro does build from
source — use it to find each other.

Every one of these packages exposes what it provides to later builds via
a `.pc` file — a plain text file recording its compiler/linker flags
(`Cflags`, `Libs`) under whatever prefix it was configured with. A
later package's build runs `pkg-config --cflags --libs libfoo` and gets
back exactly those flags, with no need to know libfoo's install layout
itself. Two environment variables make this work against a sysroot
instead of a real system install: `PKG_CONFIG_PATH` points pkg-config at
the sysroot's own `.pc` file directory instead of the host's, and
`PKG_CONFIG_SYSROOT_DIR` rewrites the `-I`/`-L` paths those `.pc` files
record — written as if `/usr/...` were real — into the sysroot's actual,
on-disk `<sysroot>/usr/...`, since the `.pc` files themselves were
generated assuming a real install, exactly per the `DESTDIR` convention
above.

=== wayland's one exception, on purpose

One package in this chain is deliberately built *without* `DESTDIR` at
all: `wayland` itself is configured with a real, absolute
`--prefix=<sysroot>/usr` and installed directly. `wayland-scanner`'s own
path is recorded in `wayland`'s `.pc` file as a *custom* pkg-config
variable (`wayland_scanner=${bindir}/wayland-scanner`) — and unlike the
ordinary `Cflags`/`Libs` fields, `PKG_CONFIG_SYSROOT_DIR` does not
rewrite custom variables, because pkg-config has no way to know a given
custom variable even represents a path. `wayland-protocols`' build reads
that variable and runs whatever it names, literally, as part of its own
build — with a plain `--prefix=/usr` and `DESTDIR`, that would resolve
to `/usr/bin/wayland-scanner`, a path that exists nowhere on the actual
build host. Building `wayland` against its own real, final sysroot path
instead sidesteps the problem entirely: this is safe specifically
*because* none of `wayland`'s own shared libraries do a prefix-derived
runtime lookup the way some of these packages' daemons do — its `.so`s
don't care what prefix they think they're under, only the one
`wayland-scanner` invocation during a later package's build does.

=== Dependency order: what has to build before what

None of this — `DESTDIR`, `PKG_CONFIG_SYSROOT_DIR`, `wayland`'s own
exception — works if a package builds before whatever it needs is
already sitting in the sysroot. With around twenty packages needing each
other in varying combinations, that order isn't hand-sequenced: each
buildpack declares its own dependencies, and a real topological sort
(§8.1) computes an order that satisfies every one of them, re-rendered
to `dependency-graph.svg` on every build (§8.1) — what follows is a
couple of representative edges out of that full graph, not the graph in
full.

Mesa is the deepest single node in it — it can't build until *six*
other packages already have:

#dtable(
  columns: (auto, 1fr),
  ([Package], [Depends on]),
  ([Mesa], [`libdrm`, `wayland`, `libxkbcommon`, `pixman`, `libdisplay-info`, `libinput`]),
  ([Weston], [`Mesa` (via `libdrm`/EGL), `libinput`, `wayland`, `wayland-protocols`, `libxkbcommon`, `cairo`, `xkeyboard-config`]),
)

One concrete path through the graph, start to finish — illustrative, not
exhaustive; `libinput` alone also needs `libevdev`, omitted here to keep
the chain readable:

#flow("eudev", "libinput", "Mesa", "Weston")

`eudev` has no sysroot dependencies of its own (§10.3.2 covers why it's
needed at all — `libinput`'s hard `libudev` dependency), which is what
lets it build first; everything downstream of it in this particular
chain literally cannot start until `eudev`'s own `.pc` file exists in
the sysroot for `PKG_CONFIG_SYSROOT_DIR` to find.

Assembling the rootfs copies this whole sysroot tree in with one
`cp -a` — every package that built into it, regardless of which
mechanism (`DESTDIR` or a real sysroot prefix) actually put it there.

== Rust on target: a working toolchain, not just `rustc --version`

Every other use of Rust in this whole project up to this point is the
*host's* own toolchain compiling buildpacks like `distro-init`. This is
different: `rustc`/`cargo` genuinely running *on the built image*,
`§11`'s "next phase" for a while. Building `rustc` from source is a
multi-hour, multi-stage bootstrap — not practical here, so
`rust_toolchain` follows the same precedent as glibc's shared libraries
and locale/Compose data (`rootfs.rs`'s `HOST_DYNAMIC_LIBS`/
`install_locale_data`): download the official prebuilt
binary distribution from `static.rust-lang.org` and place its files via
`rust-installer`'s own `install.sh`, `rust-docs`/`rust-docs-json-preview`
excluded (dead weight on this image). `distro` only — official Rust has
no native musl-hosted `rustc` at all (only `cargo` ships an
`x86_64-unknown-linux-musl` host build; `rust-std` for musl exists
solely as a cross-compilation target), a hard blocker for `distroless`,
not a scope choice.

#callout(kind: "trap", "rustc --version worked immediately; actually compiling didn't")[
  `rustc`/`cargo` both ran and reported their versions the moment the
  tarball's files landed in the rootfs. Compiling anything failed
  outright: `error: linker `cc` not found`. `rustc` still shells out to
  an external `cc` as a linker *driver* by default on
  `x86_64-unknown-linux-gnu` — confirmed against rustc's own
  documentation, not assumed — even when the actual linking backend is
  the bundled `rust-lld` (`llvm-tools-preview`, already installed).
  Full self-contained linking (no external `cc` at all) is an explicit,
  unstable, no-timeline work in progress, not something to build on. No
  C toolchain had ever existed *on* this image before — only used on the
  host to build every buildpack that needed compiling.
]

`native_gcc` closes that gap: a link-only GCC + binutils, copied
straight from the host (this project's own gcc-13/binutils, the same
"we don't build glibc from source either" reasoning as
`HOST_DYNAMIC_LIBS`/`install_locale_data`). Link-only means the
*compiler frontend* (`cc1`/`cc1plus`, ~30MB each) is deliberately left
out — `rustc` already does its own codegen and only needs `cc` to
assemble the final binary from object files it already produced: the
`gcc` driver itself, `collect2`, the real linker, gcc's own small
`crtbegin`/`crtend`/`libgcc*.a` objects, and glibc's CRT startup objects
(`crt1.o`/`crti.o`/`crtn.o`/`Scrt1.o` — from glibc, not gcc).

Getting from "files copied" to "a real `cargo build` actually links"
took three more rounds of real boot-test failures, each a genuine,
previously-invisible gap:

#callout(kind: "trap", "Round 1: ld itself needed libraries nothing else here had pulled in")[
  `ld.bfd` (real binutils, shipped alongside `rust-lld` even though
  `rustc` defaults to the latter) turned out to need `libbfd`, `libctf`,
  `libjansson`, and `libsframe` — none of them previously anywhere in
  this rootfs. Added to `HOST_DYNAMIC_LIBS`.
]

#callout(kind: "trap", "Round 2: GCC's own linker-plugin default, not rustc's fault")[
  `cc: fatal error: '-fuse-linker-plugin', but liblto_plugin.so not
  found` — this Ubuntu-packaged gcc-13's own specs (`gcc -dumpspecs`)
  unconditionally engage its linker-plugin path
  (`-plugin %(linker_plugin_file) -plugin-opt=%(lto_wrapper) ...`)
  unless `-fno-use-linker-plugin` or `-fno-lto` is passed — nothing to
  do with LTO actually being requested. `rustc`'s own default
  `-fuse-ld=lld` makes `collect2` think the linker supports the plugin
  protocol and engages it regardless. Shipping the (small, ~70KB)
  `liblto_plugin.so` got past the "not found" error, but then
  `%(lto_wrapper)` — the `lto-wrapper` program path, deliberately not
  shipped (compiler-frontend-adjacent, not needed for pure linking) —
  expanded empty: `rust-lld: error: -plugin-opt=: unknown plugin option
  ''`. Not something `rustc`'s own invocation can be changed to avoid —
  it has no idea this host's specific GCC packaging defaults this way.
  Fixed by making `cc` a thin wrapper script (`exec /usr/bin/gcc
  -fno-use-linker-plugin "$@"`) instead of a plain symlink to `gcc`.

  Writing that wrapper script surfaced a real bug of its own:
  `fs::write` follows symlinks, and the very first version of this fix
  wrote straight through the pre-existing `cc` -> `gcc` ->
  `x86_64-linux-gnu-gcc-13` symlink chain, silently overwriting the real
  compiler binary with the wrapper script's own text. Fixed by removing
  the existing path first, same defensive pattern the buildpack's own
  `symlink()` helper already used.
]

#callout(kind: "trap", "Round 3: libm.so's own linker script, and where /lib really is")[
  `rust-lld: error: cannot open /lib/x86_64-linux-gnu/libmvec.so.1: No
  such file or directory`. `libm.so` (the *development* linker script
  glibc's `-lm` resolves against, distinct from the versioned runtime
  `libm.so.6` `HOST_DYNAMIC_LIBS` already copies) references
  `libmvec.so.1` by the exact absolute path `/lib/x86_64-linux-gnu/...`
  — but `native_gcc`'s own sysroot-bulk-copied files land under
  `/usr/lib/x86_64-linux-gnu` instead, and this rootfs never merges
  `/lib` and `/usr/lib`. `libmvec` is a genuine runtime `.so.1` (not a
  dev-only stub), so the right fix was moving it into
  `HOST_DYNAMIC_LIBS` instead — the same place every other host runtime
  library it needs to sit alongside already lives.
]

Verified for real, not just "files exist": `rustc -o hello main.rs`
compiled and linked a real binary that ran and printed its own output;
`cargo init` + `cargo build` + running the resulting
`target/debug/hello_cargo` did the same through the full `cargo`
pipeline.

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
dependency order (§8.1's topological sort); `assemble-rootfs` then
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
  ("Done", [A working on-target Rust toolchain (§10.5) — `rustc`+`cargo`, verified compiling and running real binaries, not just `--version`.]),
  ("Next", [A curated Rust-CLI-tools suite (ripgrep, bat, eza, …) — trivial now that `cargo` exists on-target, each just its own tiny buildpack or a vendored `cargo install`.]),
  ("Then", [COSMIC itself, minimal subset first — see the real component breakdown below.]),
  ("Later", [The remaining COSMIC components, real GPU drivers beyond `virtio-gpu`, audio, networking UI.]),
)

COSMIC (`pop-os/cosmic-epoch`) is 28 real components, not the four-package
sketch an earlier draft of this section guessed at — pulled directly from
the project's own submodule list, not estimated:

#dtable(
  columns: (auto, 1fr),
  ([Group], [Components]),
  ([Minimal session (\~14)], [`cosmic-comp` (compositor — #strong[built, §10.3.6]), `cosmic-bg` (background — #strong[built, §10.3.6.2]), `cosmic-session` (launches/supervises the rest), `cosmic-panel`, `cosmic-applibrary`, `cosmic-launcher` + `pop-launcher` (its search backend), `cosmic-notifications`, `cosmic-osd`, `cosmic-settings-daemon`, `cosmic-idle`, `cosmic-randr`, `xdg-desktop-portal-cosmic`, `cosmic-icons`, `cosmic-term` — enough to log in (at a TTY; `cosmic-greeter` is skippable here), see a panel, and use a terminal.]),
  ([Everything else (\~14)], [`cosmic-greeter`, `cosmic-settings`, `cosmic-files`, `cosmic-edit`, `cosmic-store`, `cosmic-applets`, `cosmic-workspaces-epoch`, `cosmic-monitor`, `cosmic-screenshot`, `cosmic-theme-editor`, `cosmic-initial-setup`, `cosmic-sound-theme`, `cosmic-wallpapers` — real, but not load-bearing for "usable."]),
)

Five more — `libcosmic`, `cosmic-protocols`, `cosmic-text`, `cosmic-theme`,
`cosmic-time` — are Rust library crates these components pull in via
Cargo, not separate buildpacks of their own.

#callout(kind: "note", "A standing rule for everything above")[
  Anything that ends up *in* the built image — a buildpack's runtime
  output, a daemon, a COSMIC component, any future app — must be Rust or
  C. No Python, Ruby, Perl, or Node runtime ships on target. This is the
  same reasoning §10.3.1 already gives for choosing uutils over BusyBox,
  made explicit as a constraint on every package picked from here on —
  network stack, audio stack, and the COSMIC components above included.
  Host-side build tooling is exempt: `meson` (Python) is infrastructure,
  not distro content, the same carve-out as `gcc`/`ninja`/`pkg-config`
  (§10.4.2) — only a package's *runtime* language is constrained, never
  its build system.
]

And two deliberate gaps in what's already built, worth knowing about
rather than discovering later:

- `libxcb` was never built. Its only real consumer on a Wayland-native
  target is XWayland compatibility, not currently planned.
- `mtdev` (legacy multitouch) and `libwacom` (tablet identification) were
  both left out of `libinput` — niche hardware support, easy to add back
  later if a real device needs it.

#callout(kind: "note", "Formerly a gap: grep/sed/find/ps")[
  An earlier draft of this section listed `grep`/`sed` as a known gap —
  `uutils`' `COREUTILS_APPLETS` list had symlinked all four of `grep`,
  `sed`, `find`, and `ps` under `/bin`, but none were ever real
  coreutils applets to begin with (GNU coreutils or `uutils` — they're
  separate GNU grep/GNU sed/findutils/procps-ng projects), so every one
  of those symlinks dangled. Now fixed: real buildpacks for all four
  (`buildpacks/src/{grep,sed,findutils,procps}.rs`), verified working
  live in a booted image — see §5.4.
]
