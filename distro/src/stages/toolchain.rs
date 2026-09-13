use anyhow::{bail, Result};

/// Ensures the host build toolchain (gcc, make, glibc dev headers, ...) is
/// available for building the from-scratch userland. Not yet implemented —
/// see Phase 1 of the distro roadmap.
pub fn build_toolchain() -> Result<()> {
    bail!("build-toolchain: not yet implemented (see Phase 1 of the distro roadmap)")
}
