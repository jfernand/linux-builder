use builder_core::config::Config;
use builder_core::stages::toolchain::musl_target;
use anyhow::Result;
use std::path::PathBuf;

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

    /// Args to pass to a re-exec'd `linux-builder` child process (after the
    /// global `--config <path>` flag), running this stage non-interactively.
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

    /// On-disk outputs this stage produces. Used both to show whether the
    /// stage's work is already present and, for `clean`, what to remove to
    /// force it to redo that work. Empty for stages with nothing of their
    /// own to track: `BuildToolchain` touches global system/toolchain
    /// state, and `WriteUsb`/`TestQemu` don't persist anything under
    /// `build_dir`.
    fn output_paths(&self, cfg: &Config) -> Vec<PathBuf> {
        match self {
            StageKind::Fetch => vec![
                cfg.sources_dir(),
                cfg.kernel_build_dir(),
                cfg.busybox_build_dir(),
                cfg.uutils_build_dir(),
            ],
            StageKind::BuildToolchain => vec![],
            StageKind::BuildKernel => vec![cfg.kernel_build_dir().join("arch/x86/boot/bzImage")],
            StageKind::BuildUserland => vec![
                cfg.uutils_build_dir()
                    .join("target")
                    .join(musl_target())
                    .join("release")
                    .join("coreutils"),
                cfg.busybox_build_dir().join("busybox"),
            ],
            StageKind::AssembleRootfs => vec![cfg.rootfs_dir()],
            StageKind::MakeImage => vec![cfg.output_image()],
            StageKind::WriteUsb => vec![],
            StageKind::TestQemu => vec![],
        }
    }

    /// Whether this stage can be cleaned at all (see `output_paths`).
    pub fn can_clean(&self) -> bool {
        !matches!(self, StageKind::BuildToolchain | StageKind::WriteUsb | StageKind::TestQemu)
    }

    /// Whether every output this stage produces is already on disk.
    pub fn is_present(&self, cfg: &Config) -> bool {
        let paths = self.output_paths(cfg);
        !paths.is_empty() && paths.iter().all(|p| p.exists())
    }

    /// Remove this stage's outputs so it (and anything downstream that
    /// depends on it) will redo its work next run.
    pub fn clean(&self, cfg: &Config) -> Result<()> {
        for path in self.output_paths(cfg) {
            if path.is_dir() {
                std::fs::remove_dir_all(&path)?;
            } else if path.exists() {
                std::fs::remove_file(&path)?;
            }
        }
        Ok(())
    }
}
