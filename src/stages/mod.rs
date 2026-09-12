pub mod fetch;
pub mod toolchain;

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

/// Run a command, streaming its stdout/stderr, and error out on non-zero exit.
pub fn run(cmd: &mut Command) -> Result<()> {
    let desc = format!("{:?}", cmd);
    let status = cmd.status().with_context(|| format!("spawning {desc}"))?;
    if !status.success() {
        bail!("command failed ({status}): {desc}");
    }
    Ok(())
}

/// Run a command with a specific working directory.
pub fn run_in(dir: &Path, cmd: &mut Command) -> Result<()> {
    cmd.current_dir(dir);
    run(cmd)
}

/// Returns true if `path` already exists and `force` is false, meaning the
/// stage producing it can be skipped.
pub fn already_built(path: &Path, force: bool) -> bool {
    !force && path.exists()
}
