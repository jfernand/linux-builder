//! Process-execution primitives, ported unchanged from
//! `builder-core::stages` — `buildpack-core` doesn't depend on
//! `builder-core` (it's meant to stay foundational and independent), so
//! these are duplicated here rather than shared, deliberately small.

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

pub fn run(cmd: &mut Command) -> Result<()> {
    let status = cmd
        .status()
        .with_context(|| format!("running {cmd:?}"))?;
    if !status.success() {
        bail!("command failed ({status}): {cmd:?}");
    }
    Ok(())
}

pub fn run_in(dir: &Path, cmd: &mut Command) -> Result<()> {
    cmd.current_dir(dir);
    run(cmd)
}

pub fn already_built(path: &Path, force: bool) -> bool {
    !force && path.exists()
}
