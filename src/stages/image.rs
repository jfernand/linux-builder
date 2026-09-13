use super::{already_built, run};
use crate::config::Config;
use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

/// Builds a bootable GPT disk image (EFI System Partition + ext4 root) from
/// the assembled rootfs and kernel. Requires root (loop devices, mount,
/// grub-install) and shells out to `sudo` explicitly for those steps.
pub fn make_image(cfg: &Config, force: bool) -> Result<()> {
    let image_path = cfg.output_image();

    if already_built(&image_path, force) {
        println!("skip make-image: {} already exists", image_path.display());
        return Ok(());
    }

    create_empty_image(cfg, &image_path)?;
    partition_image(&image_path)?;

    let loop_dev = attach_loop(&image_path)?;
    let result = populate_image(cfg, &loop_dev);
    detach_loop(&loop_dev)?;
    result
}

fn create_empty_image(cfg: &Config, image_path: &Path) -> Result<()> {
    println!("creating {}MB image at {}", cfg.image.size_mb, image_path.display());
    run(Command::new("fallocate").args([
        "-l",
        &format!("{}M", cfg.image.size_mb),
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

fn populate_image(cfg: &Config, loop_dev: &str) -> Result<()> {
    let esp_part = format!("{loop_dev}p1");
    let root_part = format!("{loop_dev}p2");

    println!("formatting partitions");
    run(Command::new("sudo").args(["mkfs.vfat", "-F", "32", &esp_part]))?;
    run(Command::new("sudo").args(["mkfs.ext4", "-F", &root_part]))?;
    let root_fs_uuid = blkid_value(&root_part, "UUID")?;
    let root_part_uuid = blkid_value(&root_part, "PARTUUID")?;

    let esp_mount = cfg.build_dir.join("mnt-esp");
    let root_mount = cfg.build_dir.join("mnt-root");
    std::fs::create_dir_all(&esp_mount)?;
    std::fs::create_dir_all(&root_mount)?;

    run(Command::new("sudo").args(["mount", &root_part, root_mount.to_str().unwrap()]))?;
    let result = populate_mounted(cfg, &esp_part, &root_mount, &root_fs_uuid, &root_part_uuid);

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
    cfg: &Config,
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
        &format!("{}/.", cfg.rootfs_dir().display()),
        root_mount.to_str().unwrap(),
    ]))?;

    println!("installing kernel");
    run(Command::new("sudo").args(["mkdir", "-p", boot_dir.to_str().unwrap()]))?;
    run(Command::new("sudo").args([
        "cp",
        cfg.kernel_build_dir()
            .join("arch/x86/boot/bzImage")
            .to_str()
            .unwrap(),
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
    write_grub_cfg(&esp_target.join("boot"), &cfg.image.hostname, root_fs_uuid, root_part_uuid)?;

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
    let tmp = std::env::temp_dir().join("linux-builder-grub.cfg");
    std::fs::write(&tmp, cfg)?;
    run(Command::new("sudo").args([
        "cp",
        tmp.to_str().unwrap(),
        grub_dir.join("grub.cfg").to_str().unwrap(),
    ]))?;
    let _ = std::fs::remove_file(&tmp);
    Ok(())
}
