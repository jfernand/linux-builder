use crate::config::Config;
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

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

/// Lists removable, whole-disk block devices (i.e. plausible USB sticks),
/// excluding the disk this build is running on.
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

/// Writes the built disk image to a removable device with `dd`. Requires
/// root and refuses to run unless `confirmed` is true — callers (CLI and
/// TUI alike) are responsible for getting explicit user confirmation first,
/// since this is destructive and irreversible.
pub fn write_usb(cfg: &Config, device: &str, confirmed: bool, mut on_line: impl FnMut(&str)) -> Result<()> {
    let image = cfg.output_image();
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
