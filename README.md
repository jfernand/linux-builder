# linux-builder

A Rust CLI that orchestrates building a minimal, bootable Linux distro image
from source: a custom kernel, a musl-static userland (uutils/coreutils +
BusyBox for shell/init), and a bootable GPT disk image (EFI System Partition
+ ext4 root) that can be written to a USB stick with `dd` and booted on real
hardware (or tested first in QEMU).

## Workspace layout

Three crates:

- **`distroless`** — the musl/BusyBox-based CLI and TUI described in this
  README. Everything below (`cargo run -p distroless -- ...`) refers to it.
- **`builder-core`** — the shared build-pipeline library `distroless` (and
  eventually `distro`) is built on. Not run directly.
- **`distro`** — a scaffold for an alternative glibc + traditional-tools
  (util-linux, shadow-utils, a standalone init) build path. Not implemented
  yet; `cargo run -p distro` just prints a placeholder.

## Pipeline stages

Each stage is a subcommand and can be run independently; stages skip
re-running if their output already exists, unless `--force` is passed.

1. `fetch` — download and extract the kernel and BusyBox tarballs, and clone
   uutils/coreutils.
2. `build-toolchain` — install `musl-tools` and the `x86_64-unknown-linux-musl`
   Rust target if missing.
3. `build-kernel` — builds a minimal-base kernel config (or
   `kernel.config_file`) and `make -jN` in the kernel source tree; see
   "Customizing the kernel config" below.
4. `build-userland` — build uutils/coreutils and BusyBox, both statically
   linked against musl.
5. `assemble-rootfs` — lay out `build/rootfs` with the compiled binaries,
   symlinks, kernel modules, and minimal `/etc` config (inittab, fstab,
   init script).
6. `make-image` — partition `build/output.img` (GPT: ESP + ext4 root), copy
   the rootfs and kernel in, and install GRUB. Requires `sudo` for loop
   devices, `mkfs`, `mount`, and `grub-install`.
7. `test-qemu` — boot `build/output.img` in `qemu-system-x86_64` with OVMF
   UEFI firmware. Attaches the guest's serial console to this terminal by
   default; pass `--window` to get a normal QEMU graphical window instead
   (used by the TUI, which has no stdin to hand an interactive console).
8. `write-usb --device /dev/sdX` — write `build/output.img` to a removable
   device with `dd`. Refuses to run unless `/dev/sdX` exists, and (without
   `--yes`) prompts you to retype the device path to confirm before
   overwriting it. `list-devices` shows which removable disks are attached.

## Picking a kernel version

`linux-builder.toml`'s `[kernel]` pins an exact `version`/`url`. Instead of
hand-editing those, resolve them from kernel.org's current releases:

```bash
cargo run -p distroless -- resolve-kernel --channel stable   # latest mainline stable release
cargo run -p distroless -- resolve-kernel --channel lts      # newest maintained long-term-support branch
```

This writes the resolved `version`/`url` into the config file. Run
`fetch --clean` afterwards if you'd already downloaded a different version's
sources.

## Customizing the kernel config

By default (no `kernel.config_file` set), `build-kernel` starts from
`defconfig` and then strips it down to a minimal base by turning off every
"typical desktop" subsystem this project doesn't need (sound, Wi-Fi, legacy
NIC/PATA/PCMCIA drivers, netfilter, NFS, quotas/ACLs/SELinux, IOMMU, debug
instrumentation, 32-bit compat, and ISO9660) — everything needed to boot
(PCI, ACPI, EFI, ATA/virtio block, ext4/vfat, console) is left untouched.
Run `cargo run -p distroless -- list-features` to see the full set, each named after what
it re-enables:

```
graphics    DRM/KMS graphics + fbdev console (i915, virtio-gpu, bochs, AGP) instead of plain VGA text
sound       ALSA sound subsystem and Intel HDA driver
wireless    Wi-Fi stack (cfg80211/mac80211) and rfkill
...
```

Turn any of them on with `kernel.features` in `linux-builder.toml`:

```toml
[kernel]
features = ["graphics", "sound"]
```

or from the TUI settings screen (`s`), which lists them as checkboxes
alongside Networking. `build-kernel` re-applies `kernel.features` on every
run (including against a custom `config_file`, below), so toggling one only
takes effect on the next `--force` build.

For finer control than the named packs give you, hand-edit the config
instead:

```bash
cargo run -p distroless -- fetch          # need the kernel source extracted first
cargo run -p distroless -- menu-config    # opens `make menuconfig`; saves to ./kernel.config on exit
```

Add the printed path to `linux-builder.toml`:

```toml
[kernel]
config_file = "kernel.config"
```

and `build-kernel` will use it as-is (via `olddefconfig`, so new
kernel-version options get sane defaults) instead of `defconfig` — and skips
the minimal-base stripping above, since it's already exactly what you
picked in `menu-config`. Pass `--save-to <path>` to `menu-config` to save
elsewhere, and re-run it any time to update the saved config.

The `boot-logo` pack also takes a `kernel.logo_file`: an 80x80, ASCII (P3)
PPM with at most 224 distinct colors, replacing the stock penguin shown at
boot.

```toml
[kernel]
features = ["boot-logo"]
logo_file = "my-logo.ppm"
```

or set it from the TUI: select "Custom logo file" (right under the "Boot
logo" checkbox) on the settings screen and press `e` to browse for it.

## Interactive dashboard (TUI)

```bash
cargo run -p distroless -- tui
```

Runs every stage from an interactive dashboard instead of the command line:
arrow keys (or `j`/`k`) select a stage, `Enter` runs it with live log output
in the right-hand pane, `f` runs it with `--force` (bypassing the "already
built" skip), and `q`/`Esc` quits. "Test in QEMU" instead opens
QEMU's own graphical window (the dashboard has no stdin to hand it an
interactive serial console); the dashboard stays on that stage until you
close the window. Selecting "Write to USB" opens a
device picker (`r` to refresh) followed by a confirmation screen that
requires retyping the device path before anything is written. Some stages
need `sudo`; the TUI checks for cached/passwordless sudo on startup and, if
needed, prompts once in the plain terminal before the dashboard takes over
(spawned stages run with no stdin, so a password prompt from inside the
dashboard would hang).

`s` opens a settings screen with:

- **Networking** — persisted to `linux-builder.toml`. When on, `build-userland`
  compiles BusyBox's `udhcpc`/`ifconfig`/`route`/`ping` applets,
  `assemble-rootfs` installs a udhcpc lease script and brings up DHCP on
  `eth0` at boot, and `test-qemu` adds a NIC (QEMU user-mode NAT, with a
  built-in DHCP server — no host root needed). Off by default.
- One checkbox per kernel feature pack (see "Customizing the kernel config"
  below), persisted to `kernel.features` in `linux-builder.toml`. Off by
  default; toggling one only takes effect the next time you run that stage
  with `f`. **Custom logo file** sits right under the "Boot logo"
  checkbox: press `e` to open a directory browser (arrows to navigate,
  `Enter` on a folder to open it or `..` to go up, `Enter` on a `.ppm` file
  to pick it) instead of typing a path.
- **Hostname** is free text, not a checkbox: select it and press `e` to
  edit, `Enter` to save, `Esc` to cancel.

Run everything with:

```bash
cargo run -p distroless -- all
cargo run -p distroless -- test-qemu
```

Or run stages individually:

```bash
cargo run -p distroless -- fetch
cargo run -p distroless -- build-toolchain
cargo run -p distroless -- build-kernel
cargo run -p distroless -- build-userland
cargo run -p distroless -- assemble-rootfs
cargo run -p distroless -- make-image
cargo run -p distroless -- test-qemu
```

Configuration (kernel/BusyBox versions, image size, hostname, etc.) lives in
`linux-builder.toml`.

Boot lands on a `login:` prompt (BusyBox `getty`+`login` on both `tty1` and
the serial console) for a single `root` account with **no password** — enter
`root` and anything (or nothing) at the password prompt. Run `passwd` once
logged in to set one before exposing this to a network, especially with
`networking` on.

## Host requirements

```bash
sudo apt update
sudo apt install build-essential libncurses-dev bison flex libssl-dev \
    libelf-dev musl-tools parted dosfstools e2fsprogs grub-efi-amd64-bin \
    qemu-system-x86 ovmf wget git
```

## Writing to a real USB stick

Once `build/output.img` boots successfully in QEMU, write it to a USB drive
(replace `/dev/sdX` with your actual device — **this will erase the drive**):

```bash
cargo run -p distroless -- list-devices
cargo run -p distroless -- write-usb --device /dev/sdX
```

or equivalently, from the TUI's "Write to USB" screen.
