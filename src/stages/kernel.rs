use super::{already_built, run_in};
use crate::config::Config;
use anyhow::Result;
use std::process::Command;

pub fn build_kernel(cfg: &Config, force: bool) -> Result<()> {
    let dir = cfg.kernel_build_dir();
    let bzimage = dir.join("arch/x86/boot/bzImage");

    if already_built(&bzimage, force) {
        println!("skip build-kernel: {} already exists", bzimage.display());
        return Ok(());
    }

    println!("configuring kernel in {}", dir.display());
    run_in(&dir, Command::new("make").arg("defconfig"))?;

    let jobs = num_cpus();
    println!("building kernel ({jobs} jobs)");
    run_in(
        &dir,
        Command::new("make").arg(format!("-j{jobs}")),
    )?;

    Ok(())
}

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}
