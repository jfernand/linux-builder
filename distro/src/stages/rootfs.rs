use crate::config::Config;
use anyhow::{bail, Result};

/// Assembles the root filesystem tree: init, a real `login`/`getty` (via
/// shadow-utils/util-linux, not BusyBox's empty-password trick), and the
/// built userland. Not yet implemented — see Phase 1 of the distro
/// roadmap.
pub fn assemble_rootfs(_cfg: &Config, _force: bool) -> Result<()> {
    bail!("assemble-rootfs: not yet implemented (see Phase 1 of the distro roadmap)")
}
