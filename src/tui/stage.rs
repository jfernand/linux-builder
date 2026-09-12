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
            StageKind::TestQemu => "Test in QEMU (30s)",
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
            // Bounded so a stray un-interactive boot can't hang the
            // dashboard forever; run `linux-builder test-qemu` directly
            // for an interactive session.
            StageKind::TestQemu => vec!["test-qemu".into()],
        }
    }

    pub fn timeout_secs(&self) -> Option<u64> {
        match self {
            StageKind::TestQemu => Some(30),
            _ => None,
        }
    }
}
