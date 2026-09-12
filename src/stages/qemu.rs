use super::run;
use crate::config::Config;
use anyhow::{bail, Context, Result};
use std::process::Command;

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

pub fn test_qemu(cfg: &Config) -> Result<()> {
    let image = cfg.output_image();
    if !image.exists() {
        bail!("{} does not exist; run make-image first", image.display());
    }

    let mut cmd = Command::new("qemu-system-x86_64");
    cmd.args([
        "-m",
        "1024",
        "-drive",
        &format!("file={},format=raw,if=virtio", image.display()),
        "-nographic",
    ]);

    if let Some(ovmf_code) = OVMF_CODE_CANDIDATES
        .iter()
        .find(|p| std::path::Path::new(p).exists())
    {
        // OVMF's pflash (UEFI firmware) drives, not `-bios` (legacy SeaBIOS-
        // style blobs only, and rejects the split 4M CODE/VARS images).
        // VARS must be writable, so copy it out of the read-only source dir.
        let vars_src = OVMF_VARS_CANDIDATES
            .iter()
            .find(|p| std::path::Path::new(p).exists());
        cmd.args(["-drive", &format!("if=pflash,format=raw,readonly=on,file={ovmf_code}")]);

        if let Some(vars_src) = vars_src {
            let vars_copy = cfg.build_dir.join("ovmf-vars.fd");
            if !vars_copy.exists() {
                std::fs::copy(vars_src, &vars_copy)
                    .context("copying OVMF_VARS to a writable location")?;
            }
            cmd.args(["-drive", &format!("if=pflash,format=raw,file={}", vars_copy.display())]);
        }
    } else {
        println!("warning: no OVMF firmware found, falling back to BIOS boot (may not work with a GPT/EFI image)");
    }

    println!("booting {} in QEMU (Ctrl-A X to quit)", image.display());
    run(&mut cmd)
}
