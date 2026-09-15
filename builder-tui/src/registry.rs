//! The one seam between this crate's generic dashboard/settings UI and
//! each distro's own package set — `distro` (26 packages) and
//! `distroless` (3) have genuinely different `all_packages()`/`ctx_for`
//! implementations, so the UI takes a `Box<dyn Registry>` instead of
//! calling either directly.

use anyhow::Result;
use buildpack_core::config::DistroConfig;
use buildpack_core::{BuildCtx, Buildpack};
use buildpacks::kernel::Kernel;

pub trait Registry {
    fn all_packages(&self, cfg: &DistroConfig) -> Result<Vec<Box<dyn Buildpack>>>;
    fn ctx_for(&self, id: &str, cfg: &DistroConfig) -> BuildCtx;
    fn kernel_buildpack(&self, cfg: &DistroConfig) -> Result<Kernel>;
    fn kernel_ctx(&self, cfg: &DistroConfig) -> BuildCtx;
    fn pipeline_ctx(&self, cfg: &DistroConfig) -> Result<BuildCtx>;
    /// Path, relative to `cfg.rootfs_dir()`, that signals `assemble-rootfs`
    /// has already run (`distro`'s own init has no config files to check,
    /// so it uses `sbin/init`; `distroless`'s BusyBox init reads
    /// `etc/inittab`).
    fn rootfs_ready_marker(&self) -> &'static str;
}
