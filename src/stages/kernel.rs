use super::{already_built, run_in};
use crate::config::Config;
use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

pub fn build_kernel(cfg: &Config, force: bool) -> Result<()> {
    let dir = cfg.kernel_build_dir();
    let bzimage = dir.join("arch/x86/boot/bzImage");

    if already_built(&bzimage, force) {
        println!("skip build-kernel: {} already exists", bzimage.display());
        return Ok(());
    }

    configure_kernel(cfg, &dir)?;

    let jobs = num_cpus();
    println!("building kernel ({jobs} jobs)");
    run_in(
        &dir,
        Command::new("make").arg(format!("-j{jobs}")),
    )?;

    Ok(())
}

/// Seeds the kernel's `.config`: from `kernel.config_file` if one is set
/// (re-resolved with `olddefconfig` in case it predates a kernel upgrade),
/// otherwise the stock `defconfig`.
fn configure_kernel(cfg: &Config, dir: &Path) -> Result<()> {
    match &cfg.kernel.config_file {
        Some(path) => {
            println!("applying custom kernel config from {}", path.display());
            std::fs::copy(path, dir.join(".config")).with_context(|| {
                format!("copying {} into {}", path.display(), dir.display())
            })?;
            run_in(dir, Command::new("make").arg("olddefconfig"))?;
        }
        None => {
            println!("configuring kernel in {}", dir.display());
            run_in(dir, Command::new("make").arg("defconfig"))?;
        }
    }
    Ok(())
}

/// Interactively runs `make menuconfig` against the kernel sources so
/// options can be toggled by hand, then saves the resulting `.config` to
/// `save_to` for reuse as `kernel.config_file`.
pub fn menuconfig(cfg: &Config, save_to: &Path) -> Result<()> {
    let dir = cfg.kernel_build_dir();
    if !dir.exists() {
        bail!(
            "kernel sources not found at {} — run `fetch` first",
            dir.display()
        );
    }

    if !dir.join(".config").exists() {
        configure_kernel(cfg, &dir)?;
    }

    run_in(&dir, Command::new("make").arg("menuconfig"))?;

    if let Some(parent) = save_to.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(dir.join(".config"), save_to)
        .with_context(|| format!("saving kernel config to {}", save_to.display()))?;

    println!("saved kernel config to {}", save_to.display());
    println!(
        "set kernel.config_file = \"{}\" in your config file to build with it",
        save_to.display()
    );

    Ok(())
}

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}
