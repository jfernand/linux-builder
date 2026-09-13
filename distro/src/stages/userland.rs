use crate::config::Config;
use anyhow::{bail, Result};

/// Builds the glibc userland from source: uutils/coreutils (native
/// `x86_64-unknown-linux-gnu`), util-linux, shadow-utils. Not yet
/// implemented — see Phase 1 of the distro roadmap.
pub fn build_userland(_cfg: &Config, _force: bool) -> Result<()> {
    bail!("build-userland: not yet implemented (see Phase 1 of the distro roadmap)")
}
