//! Operations that act on the whole assembled distro rather than any one
//! package — `make-image`, `test-qemu`, `write-usb` — ported unchanged
//! from `builder-core/src/stages/{image,qemu,usb}.rs`. Not `Buildpack`s:
//! they have no source of their own and produce no `BuildOutput`. Unlike
//! `AssembleRootfs`/`Toolchain`, these three are byte-for-byte identical
//! logic between `distro` and `distroless` today, so they're shared here
//! instead of duplicated per distro.

use crate::run::run;
use crate::BuildCtx;
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

pub trait PipelineStage {
    fn id(&self) -> &'static str;
    fn run(&self, ctx: &BuildCtx, force: bool) -> Result<()>;
    /// Is this stage's output already present? Default `false` — most of
    /// these have no single persisted marker (`TestQemu`/`WriteUsb` are
    /// actions, re-run every time), matching today's TUI behavior.
    fn is_done(&self, _ctx: &BuildCtx) -> bool {
        false
    }
}

// --- make-image ------------------------------------------------------

pub struct MakeImage;

impl PipelineStage for MakeImage {
    fn id(&self) -> &'static str {
        "make_image"
    }

    fn run(&self, ctx: &BuildCtx, force: bool) -> Result<()> {
        make_image(ctx, force)
    }

    fn is_done(&self, ctx: &BuildCtx) -> bool {
        output_image(ctx).exists()
    }
}

fn output_image(ctx: &BuildCtx) -> std::path::PathBuf {
    ctx.build_dir.join("output.img")
}

/// Builds a bootable GPT disk image (EFI System Partition + ext4 root)
/// from the assembled rootfs and kernel. Requires root (loop devices,
/// mount, grub-install) and shells out to `sudo` explicitly for those
/// steps.
fn make_image(ctx: &BuildCtx, force: bool) -> Result<()> {
    let image_path = output_image(ctx);

    if crate::run::already_built(&image_path, force) {
        println!("skip make-image: {} already exists", image_path.display());
        return Ok(());
    }

    create_empty_image(ctx, &image_path)?;
    partition_image(&image_path)?;

    let loop_dev = attach_loop(&image_path)?;
    let result = populate_image(ctx, &loop_dev);
    detach_loop(&loop_dev)?;
    result
}

fn create_empty_image(ctx: &BuildCtx, image_path: &Path) -> Result<()> {
    println!("creating {}MB image at {}", ctx.image.size_mb, image_path.display());
    run(Command::new("fallocate").args([
        "-l",
        &format!("{}M", ctx.image.size_mb),
        image_path.to_str().unwrap(),
    ]))
}

fn partition_image(image_path: &Path) -> Result<()> {
    let path = image_path.to_str().unwrap();
    println!("partitioning {path} (GPT: ESP + ext4 root)");
    run(Command::new("parted").args(["-s", path, "mklabel", "gpt"]))?;
    run(Command::new("parted").args([
        "-s", path, "mkpart", "ESP", "fat32", "1MiB", "257MiB",
    ]))?;
    run(Command::new("parted").args(["-s", path, "set", "1", "esp", "on"]))?;
    run(Command::new("parted").args([
        "-s", path, "mkpart", "root", "ext4", "257MiB", "100%",
    ]))?;
    Ok(())
}

fn attach_loop(image_path: &Path) -> Result<String> {
    let output = Command::new("sudo")
        .args(["losetup", "-Pf", "--show", image_path.to_str().unwrap()])
        .output()
        .context("running losetup")?;
    if !output.status.success() {
        bail!("losetup failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn detach_loop(loop_dev: &str) -> Result<()> {
    run(Command::new("sudo").args(["losetup", "-d", loop_dev]))
}

fn populate_image(ctx: &BuildCtx, loop_dev: &str) -> Result<()> {
    let esp_part = format!("{loop_dev}p1");
    let root_part = format!("{loop_dev}p2");

    println!("formatting partitions");
    run(Command::new("sudo").args(["mkfs.vfat", "-F", "32", &esp_part]))?;
    run(Command::new("sudo").args(["mkfs.ext4", "-F", &root_part]))?;
    let root_fs_uuid = blkid_value(&root_part, "UUID")?;
    let root_part_uuid = blkid_value(&root_part, "PARTUUID")?;

    let esp_mount = ctx.build_dir.join("mnt-esp");
    let root_mount = ctx.build_dir.join("mnt-root");
    std::fs::create_dir_all(&esp_mount)?;
    std::fs::create_dir_all(&root_mount)?;

    run(Command::new("sudo").args(["mount", &root_part, root_mount.to_str().unwrap()]))?;
    let result = populate_mounted(ctx, &esp_part, &root_mount, &root_fs_uuid, &root_part_uuid);

    // Always attempt cleanup, even if population failed partway through.
    let _ = Command::new("sudo")
        .args(["umount", "-R", root_mount.to_str().unwrap()])
        .status();

    result
}

fn blkid_value(partition: &str, tag: &str) -> Result<String> {
    let output = Command::new("sudo")
        .args(["blkid", "-s", tag, "-o", "value", partition])
        .output()
        .context("running blkid")?;
    if !output.status.success() {
        bail!("blkid -s {tag} failed for {partition}: {}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn populate_mounted(
    ctx: &BuildCtx,
    esp_part: &str,
    root_mount: &Path,
    root_fs_uuid: &str,
    root_part_uuid: &str,
) -> Result<()> {
    let boot_dir = root_mount.join("boot");
    // Staging mountpoint for the ESP so we can copy files onto it; this is
    // NOT where the ESP lives at boot time (it's its own top-level
    // partition), it's just where we write into it during image population.
    let esp_target = boot_dir.join("efi");
    run(Command::new("sudo").args(["mkdir", "-p", esp_target.to_str().unwrap()]))?;
    run(Command::new("sudo").args(["mount", esp_part, esp_target.to_str().unwrap()]))?;

    println!("copying rootfs into image");
    run(Command::new("sudo").args([
        "cp",
        "-a",
        &format!("{}/.", ctx.rootfs_dir.display()),
        root_mount.to_str().unwrap(),
    ]))?;

    println!("installing kernel");
    run(Command::new("sudo").args(["mkdir", "-p", boot_dir.to_str().unwrap()]))?;
    run(Command::new("sudo").args([
        "cp",
        ctx.kernel_bzimage.to_str().unwrap(),
        boot_dir.join("vmlinuz").to_str().unwrap(),
    ]))?;

    println!("installing grub");
    run(Command::new("sudo").args([
        "grub-install",
        "--target=x86_64-efi",
        &format!("--efi-directory={}", esp_target.display()),
        &format!("--boot-directory={}", boot_dir.display()),
        "--removable",
    ]))?;

    // grub-install's --removable core image resolves its prefix against the
    // ESP itself (the partition it was loaded from), not --boot-directory,
    // so the grub.cfg that's actually read at boot time must live under
    // <ESP>/boot/grub/grub.cfg, not <root>/boot/grub/grub.cfg.
    write_grub_cfg(&esp_target.join("boot"), &ctx.image.hostname, root_fs_uuid, root_part_uuid)?;

    Ok(())
}

fn write_grub_cfg(
    grub_boot_dir: &Path,
    hostname: &str,
    root_fs_uuid: &str,
    root_part_uuid: &str,
) -> Result<()> {
    let grub_dir = grub_boot_dir.join("grub");
    run(Command::new("sudo").args(["mkdir", "-p", grub_dir.to_str().unwrap()]))?;

    // GRUB locates the kernel file by filesystem UUID; the kernel itself
    // (no initramfs here) only understands PARTUUID= natively for root=, not
    // filesystem UUID= (that resolution normally happens in userspace/initrd).
    let cfg = format!(
        "set timeout=3\n\
         menuentry \"{hostname}\" {{\n\
         \tsearch --no-floppy --fs-uuid --set=root {root_fs_uuid}\n\
         \tlinux ($root)/boot/vmlinuz root=PARTUUID={root_part_uuid} rw console=tty0 console=ttyS0,115200\n\
         }}\n"
    );
    let tmp = std::env::temp_dir().join("distroless-grub.cfg");
    std::fs::write(&tmp, cfg)?;
    run(Command::new("sudo").args([
        "cp",
        tmp.to_str().unwrap(),
        grub_dir.join("grub.cfg").to_str().unwrap(),
    ]))?;
    let _ = std::fs::remove_file(&tmp);
    Ok(())
}

// --- test-qemu ---------------------------------------------------------

const OVMF_CODE_CANDIDATES: &[&str] = &[
    "/usr/share/OVMF/OVMF_CODE_4M.fd",
    "/usr/share/OVMF/OVMF_CODE.fd",
    "/usr/share/ovmf/OVMF_CODE.fd",
    "/usr/share/edk2/ovmf/OVMF_CODE.fd",
];

const OVMF_VARS_CANDIDATES: &[&str] = &[
    "/usr/share/OVMF/OVMF_VARS_4M.fd",
    "/usr/share/OVMF/OVMF_VARS.fd",
    "/usr/share/ovmf/OVMF_VARS.fd",
    "/usr/share/edk2/ovmf/OVMF_VARS.fd",
];

pub struct TestQemu {
    pub window: bool,
}

impl PipelineStage for TestQemu {
    fn id(&self) -> &'static str {
        "test_qemu"
    }

    fn run(&self, ctx: &BuildCtx, _force: bool) -> Result<()> {
        test_qemu(ctx, self.window)
    }
}

fn test_qemu(ctx: &BuildCtx, window: bool) -> Result<()> {
    let image = output_image(ctx);
    if !image.exists() {
        bail!("{} does not exist; run make-image first", image.display());
    }

    let mut cmd = Command::new("qemu-system-x86_64");
    cmd.args([
        "-m",
        "1024",
        "-drive",
        &format!("file={},format=raw,if=virtio", image.display()),
    ]);

    if window {
        // Let QEMU open its own graphical window (its default display
        // backend) with its own keyboard focus, instead of attaching the
        // guest's serial console to our stdio.
    } else {
        cmd.arg("-nographic");
    }

    if ctx.networking {
        // Unprivileged user-mode NAT with a built-in DHCP server, so
        // udhcpc in the guest has something to talk to with no host-side
        // root or network config needed.
        cmd.args(["-netdev", "user,id=n0", "-device", "virtio-net-pci,netdev=n0"]);
    }

    if let Some(ovmf_code) = OVMF_CODE_CANDIDATES.iter().find(|p| Path::new(p).exists()) {
        // OVMF's pflash (UEFI firmware) drives, not `-bios` (legacy SeaBIOS-
        // style blobs only, and rejects the split 4M CODE/VARS images).
        // VARS must be writable, so copy it out of the read-only source dir.
        let vars_src = OVMF_VARS_CANDIDATES.iter().find(|p| Path::new(p).exists());
        cmd.args(["-drive", &format!("if=pflash,format=raw,readonly=on,file={ovmf_code}")]);

        if let Some(vars_src) = vars_src {
            let vars_copy = ctx.build_dir.join("ovmf-vars.fd");
            if !vars_copy.exists() {
                std::fs::copy(vars_src, &vars_copy)
                    .context("copying OVMF_VARS to a writable location")?;
            }
            cmd.args(["-drive", &format!("if=pflash,format=raw,file={}", vars_copy.display())]);
        }
    } else {
        println!("warning: no OVMF firmware found, falling back to BIOS boot (may not work with a GPT/EFI image)");
    }

    if window {
        println!("booting {} in QEMU (close the window to quit)", image.display());
    } else {
        println!("booting {} in QEMU (Ctrl-A X to quit)", image.display());
    }
    run(&mut cmd)
}

// --- write-usb + device listing -----------------------------------------

#[derive(Debug, Deserialize)]
struct RawDevice {
    name: String,
    size: u64,
    model: Option<String>,
    tran: Option<String>,
    #[serde(default)]
    rm: bool,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Debug, Clone)]
pub struct Device {
    pub name: String,
    pub size: String,
    pub model: String,
    pub tran: String,
}

impl Device {
    pub fn path(&self) -> String {
        format!("/dev/{}", self.name)
    }
}

#[derive(Deserialize)]
struct LsblkOutput {
    blockdevices: Vec<RawDevice>,
}

/// Lists removable, whole-disk block devices (i.e. plausible USB sticks).
/// Not a `PipelineStage` itself — a plain helper `WriteUsb`'s caller uses
/// to get a device list for its confirmation prompt.
pub fn list_removable_devices() -> Result<Vec<Device>> {
    let output = Command::new("lsblk")
        .args(["-J", "-b", "-o", "NAME,SIZE,MODEL,TRAN,RM,TYPE"])
        .output()
        .context("running lsblk")?;
    if !output.status.success() {
        bail!("lsblk failed: {}", String::from_utf8_lossy(&output.stderr));
    }

    let parsed: LsblkOutput =
        serde_json::from_slice(&output.stdout).context("parsing lsblk JSON output")?;

    let devices: Vec<Device> = parsed
        .blockdevices
        .into_iter()
        .filter(|d| d.kind == "disk" && d.rm)
        .map(|d| Device {
            name: d.name,
            size: humanize_bytes(d.size),
            model: d.model.unwrap_or_default().trim().to_string(),
            tran: d.tran.unwrap_or_default(),
        })
        .collect();

    Ok(devices)
}

fn humanize_bytes(bytes: u64) -> String {
    let mut value = bytes as f64;
    for unit in ["B", "KiB", "MiB", "GiB", "TiB"] {
        if value < 1024.0 {
            return format!("{value:.1}{unit}");
        }
        value /= 1024.0;
    }
    format!("{value:.1}PiB")
}

pub struct WriteUsb {
    pub device: String,
    pub confirmed: bool,
}

impl PipelineStage for WriteUsb {
    fn id(&self) -> &'static str {
        "write_usb"
    }

    fn run(&self, ctx: &BuildCtx, _force: bool) -> Result<()> {
        write_usb(ctx, &self.device, self.confirmed, |line| println!("{line}"))
    }
}

/// Writes the built disk image to a removable device with `dd`. Requires
/// root and refuses to run unless `confirmed` is true — callers (CLI and
/// TUI alike) are responsible for getting explicit user confirmation
/// first, since this is destructive and irreversible.
pub fn write_usb(ctx: &BuildCtx, device: &str, confirmed: bool, mut on_line: impl FnMut(&str)) -> Result<()> {
    let image = output_image(ctx);
    if !image.exists() {
        bail!("{} does not exist; run make-image first", image.display());
    }

    if !Path::new(device).exists() {
        bail!("device {device} does not exist");
    }

    if !confirmed {
        bail!("refusing to write to {device} without confirmation (pass --yes after verifying the device)");
    }

    let mut child = Command::new("sudo")
        .args([
            "dd",
            &format!("if={}", image.display()),
            &format!("of={device}"),
            "bs=4M",
            "status=progress",
            "conv=fsync",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawning dd")?;

    // `dd status=progress` overwrites its progress report in place using
    // '\r', only emitting a real '\n' for the final summary line, so split
    // on either to surface each progress update as it happens.
    let mut stderr = child.stderr.take().expect("piped stderr");
    let mut buf = [0u8; 4096];
    let mut pending = String::new();
    loop {
        let n = stderr.read(&mut buf).context("reading dd stderr")?;
        if n == 0 {
            break;
        }
        pending.push_str(&String::from_utf8_lossy(&buf[..n]));
        while let Some(pos) = pending.find(['\r', '\n']) {
            let line = pending[..pos].to_string();
            pending.drain(..=pos);
            if !line.is_empty() {
                on_line(&line);
            }
        }
    }
    if !pending.is_empty() {
        on_line(&pending);
    }

    let status = child.wait().context("waiting for dd")?;
    if !status.success() {
        bail!("dd failed with {status}");
    }

    Ok(())
}
