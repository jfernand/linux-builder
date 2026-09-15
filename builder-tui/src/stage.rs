use crate::registry::Registry;
use anyhow::Result;
use buildpack_core::config::DistroConfig;
use buildpack_core::Buildpack;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StageKind {
    Fetch,
    BuildToolchain,
    BuildKernel,
    BuildUserland,
    AssembleRootfs,
    MakeImage,
    WriteUsb,
    TestQemu,
}

pub const STAGES: [StageKind; 8] = [
    StageKind::Fetch,
    StageKind::BuildToolchain,
    StageKind::BuildKernel,
    StageKind::BuildUserland,
    StageKind::AssembleRootfs,
    StageKind::MakeImage,
    StageKind::WriteUsb,
    StageKind::TestQemu,
];

impl StageKind {
    pub fn label(&self) -> &'static str {
        match self {
            StageKind::Fetch => "Fetch sources",
            StageKind::BuildToolchain => "Build toolchain",
            StageKind::BuildKernel => "Build kernel",
            StageKind::BuildUserland => "Build userland",
            StageKind::AssembleRootfs => "Assemble rootfs",
            StageKind::MakeImage => "Make image",
            StageKind::WriteUsb => "Write to USB",
            StageKind::TestQemu => "Test in QEMU (opens a window)",
        }
    }

    pub fn needs_device(&self) -> bool {
        matches!(self, StageKind::WriteUsb)
    }

    /// Args to pass to a re-exec'd distro binary (after the global
    /// `--config <path>` flag), running this stage non-interactively.
    pub fn subcommand_args(&self, device: Option<&str>) -> Vec<String> {
        match self {
            StageKind::Fetch => vec!["fetch".into()],
            StageKind::BuildToolchain => vec!["build-toolchain".into()],
            StageKind::BuildKernel => vec!["build-kernel".into()],
            StageKind::BuildUserland => vec!["build-userland".into()],
            StageKind::AssembleRootfs => vec!["assemble-rootfs".into()],
            StageKind::MakeImage => vec!["make-image".into()],
            StageKind::WriteUsb => vec![
                "write-usb".into(),
                "--device".into(),
                device.unwrap_or_default().into(),
                "--yes".into(),
            ],
            // `--window`: the dashboard has no stdin to hand an
            // interactive serial console, so open QEMU's own graphical
            // window instead.
            StageKind::TestQemu => vec!["test-qemu".into(), "--window".into()],
        }
    }

    /// Whether this stage can be cleaned at all.
    pub fn can_clean(&self) -> bool {
        !matches!(self, StageKind::BuildToolchain | StageKind::WriteUsb | StageKind::TestQemu)
    }

    /// Whether every output this stage produces is already on disk.
    /// `Fetch`/`BuildKernel`/`BuildUserland` delegate to the real
    /// `Buildpack::is_built` for each package involved (via the
    /// `Registry`); `AssembleRootfs`/`MakeImage` check `DistroConfig`'s
    /// own path helpers directly, since neither has a `Buildpack`/
    /// `PipelineStage` of its own to delegate to;
    /// `BuildToolchain`/`WriteUsb`/`TestQemu` have no persisted state.
    pub fn is_present(&self, cfg: &DistroConfig, reg: &dyn Registry) -> bool {
        match self {
            StageKind::Fetch => {
                let Ok(packs) = reg.all_packages(cfg) else { return false };
                !packs.is_empty() && packs.iter().all(|p| reg.ctx_for(p.id(), cfg).sources_dir.exists())
            }
            StageKind::BuildToolchain => false,
            StageKind::BuildKernel => {
                let Ok(kernel) = reg.kernel_buildpack(cfg) else { return false };
                kernel.is_built(&reg.kernel_ctx(cfg))
            }
            StageKind::BuildUserland => {
                let Ok(packs) = reg.all_packages(cfg) else { return false };
                packs.iter().filter(|p| p.id() != "kernel").all(|p| p.is_built(&reg.ctx_for(p.id(), cfg)))
            }
            StageKind::AssembleRootfs => cfg.rootfs_dir().join(reg.rootfs_ready_marker()).exists(),
            StageKind::MakeImage => cfg.output_image().exists(),
            StageKind::WriteUsb | StageKind::TestQemu => false,
        }
    }

    /// Remove this stage's outputs so it (and anything downstream that
    /// depends on it) will redo its work next run.
    pub fn clean(&self, cfg: &DistroConfig, reg: &dyn Registry) -> Result<()> {
        match self {
            StageKind::Fetch => {
                for pack in reg.all_packages(cfg)? {
                    let dir = reg.ctx_for(pack.id(), cfg).sources_dir;
                    if dir.exists() {
                        std::fs::remove_dir_all(&dir)?;
                    }
                }
            }
            StageKind::BuildKernel => {
                reg.kernel_buildpack(cfg)?.clean(&reg.kernel_ctx(cfg))?;
            }
            StageKind::BuildUserland => {
                for pack in reg.all_packages(cfg)?.iter().filter(|p| p.id() != "kernel") {
                    pack.clean(&reg.ctx_for(pack.id(), cfg))?;
                }
            }
            StageKind::AssembleRootfs => {
                let dir = cfg.rootfs_dir();
                if dir.exists() {
                    std::fs::remove_dir_all(&dir)?;
                }
            }
            StageKind::MakeImage => {
                let path = cfg.output_image();
                if path.exists() {
                    std::fs::remove_file(&path)?;
                }
            }
            StageKind::BuildToolchain | StageKind::WriteUsb | StageKind::TestQemu => {}
        }
        Ok(())
    }
}
