# linux-builder

A Rust CLI that orchestrates building a minimal, bootable Linux distro image
from source: a custom kernel, a musl-static userland (uutils/coreutils +
BusyBox for shell/init), and a bootable GPT disk image (EFI System Partition
+ ext4 root) that can be written to a USB stick with `dd` and booted on real
hardware (or tested first in QEMU).

## Pipeline stages

Each stage is a subcommand and can be run independently; stages skip
re-running if their output already exists, unless `--force` is passed.

1. `fetch` — download and extract the kernel and BusyBox tarballs, and clone
   uutils/coreutils.
2. `build-toolchain` — install `musl-tools` and the `x86_64-unknown-linux-musl`
   Rust target if missing.
3. `build-kernel` — `make defconfig && make -jN` in the kernel source tree.
4. `build-userland` — build uutils/coreutils and BusyBox, both statically
   linked against musl.
5. `assemble-rootfs` — lay out `build/rootfs` with the compiled binaries,
   symlinks, kernel modules, and minimal `/etc` config (inittab, fstab,
   init script).
6. `make-image` — partition `build/output.img` (GPT: ESP + ext4 root), copy
   the rootfs and kernel in, and install GRUB. Requires `sudo` for loop
   devices, `mkfs`, `mount`, and `grub-install`.
7. `test-qemu` — boot `build/output.img` in `qemu-system-x86_64` with OVMF
   UEFI firmware.
8. `write-usb --device /dev/sdX` — write `build/output.img` to a removable
   device with `dd`. Refuses to run unless `/dev/sdX` exists, and (without
   `--yes`) prompts you to retype the device path to confirm before
   overwriting it. `list-devices` shows which removable disks are attached.

Run everything with:

```bash
cargo run -- all
cargo run -- test-qemu
```

Or run stages individually:

```bash
cargo run -- fetch
cargo run -- build-toolchain
cargo run -- build-kernel
cargo run -- build-userland
cargo run -- assemble-rootfs
cargo run -- make-image
cargo run -- test-qemu
```

Configuration (kernel/BusyBox versions, image size, hostname, etc.) lives in
`linux-builder.toml`.

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
cargo run -- list-devices
cargo run -- write-usb --device /dev/sdX
```
