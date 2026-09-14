//! uutils/coreutils — a Rust reimplementation of GNU coreutils, one
//! multi-call binary symlinked under every applet name.

use anyhow::{bail, Context, Result};
use buildpack_core::build::{cargo_build_release, cargo_target_dir};
use buildpack_core::run::already_built;
use buildpack_core::{
    BuildCtx, BuildOutput, Buildpack, Description, InstallMode, RootfsInstall, Source, SourcePatch,
};
use serde::Deserialize;
use std::any::Any;
use std::path::{Path, PathBuf};

const GNU_TARGET: &str = "x86_64-unknown-linux-gnu";
const MUSL_TARGET: &str = "x86_64-unknown-linux-musl";

/// Not exhaustive, just enough for a usable minimal shell environment.
/// `grep`/`sed` are listed but dangle: neither was ever part of
/// coreutils' scope upstream in uutils, despite the names being tempting
/// to include here. Identical between both variants — uutils supports
/// the same applets regardless of target libc.
const COREUTILS_APPLETS: &[&str] = &[
    "ls", "cat", "cp", "mv", "rm", "mkdir", "rmdir", "echo", "pwd", "touch", "chmod", "chown",
    "ln", "grep", "sed", "head", "tail", "sort", "uniq", "wc", "find", "env", "true", "false",
    "test", "[", "df", "du", "date", "uname", "sleep", "kill", "ps",
];

/// `distro` (glibc) and `distroless` (musl) both use uutils, with
/// genuinely different build logic — target triple, cargo feature set,
/// and static-linking mechanism (glibc needs an explicit
/// `RUSTFLAGS=-C target-feature=+crt-static`; the musl target is static
/// by default) — but everything else about the package (source, patch,
/// applet list, install shape) is identical. One buildpack with a
/// variant, not two.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum UutilsVariant {
    #[default]
    Glibc,
    Musl,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UutilsConfig {
    pub git_url: String,
    pub git_rev: String,
}

#[derive(Default)]
pub struct Uutils {
    cfg: UutilsConfig,
    variant: UutilsVariant,
}

impl Uutils {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn musl() -> Self {
        Self { variant: UutilsVariant::Musl, ..Self::default() }
    }

    /// Flips an already-`configure()`d instance to the musl variant —
    /// for callers using the generic `T::default()` + `configure()`
    /// construction pattern, where `Self::musl()` isn't reachable directly.
    pub fn into_musl(mut self) -> Self {
        self.variant = UutilsVariant::Musl;
        self
    }

    fn target(&self) -> &'static str {
        match self.variant {
            UutilsVariant::Glibc => GNU_TARGET,
            UutilsVariant::Musl => MUSL_TARGET,
        }
    }

    fn checkout_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join("uutils")
    }
}

impl Buildpack for Uutils {
    fn id(&self) -> &'static str {
        "uutils"
    }

    fn configure(&mut self, table: &toml::Value) -> Result<()> {
        self.cfg = table.clone().try_into().context("parsing [uutils] config")?;
        Ok(())
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    fn describe(&self) -> Description {
        Description {
            id: "uutils",
            name: "uutils/coreutils",
            summary: "GNU coreutils reimplemented in Rust — one multi-call binary",
            long_description: match self.variant {
                UutilsVariant::Glibc => "cargo build --release, native x86_64-unknown-linux-gnu \
                    (no cross-compilation), statically linked via RUSTFLAGS.",
                UutilsVariant::Musl => "cargo build --release, cross-compiled to \
                    x86_64-unknown-linux-musl, static by default.",
            },
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Git {
            url: self.cfg.git_url.clone(),
            rev: self.cfg.git_rev.clone(),
            checkout_dir_name: "uutils".to_string(),
        }]
    }

    fn patches(&self, _ctx: &BuildCtx) -> Vec<SourcePatch> {
        vec![SourcePatch {
            description: "AT_EXECFN empty-fallback workaround in validation.rs",
            apply: patch_binary_path,
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> Result<()> {
        let dir = self.checkout_dir(ctx);
        let target = self.target();
        let binary = cargo_target_dir(&dir).join(target).join("release").join("coreutils");

        if already_built(&binary, force) {
            println!("skip build-uutils: {} already exists", binary.display());
            return Ok(());
        }

        println!("building uutils/coreutils for {target} (static)");
        match self.variant {
            UutilsVariant::Glibc => cargo_build_release(
                &dir,
                target,
                &[
                    "--no-default-features",
                    "--features",
                    // See distro/src/stages/userland.rs's old build_uutils
                    // for why `stty`/`stdbuf` are excluded: neither builds
                    // under plain static linking (stdbuf needs a cdylib;
                    // stty's versioned glibc symbols don't resolve
                    // statically).
                    "feat_common_core,arch,kill,hostname,hostid,nohup,nproc,sync,timeout,uname,\
                     uptime,whoami,\
                     chgrp,chmod,chown,chroot,groups,id,install,logname,mkfifo,mknod,stat,\
                     pinky,users,who",
                ],
                // Static linking needs an explicit crt-static request on
                // glibc; the musl target is static by default and needs no
                // equivalent.
                &[("RUSTFLAGS", "-C target-feature=+crt-static")],
            ),
            UutilsVariant::Musl => cargo_build_release(
                &dir,
                target,
                &["--no-default-features", "--features", "feat_os_unix_musl"],
                &[],
            ),
        }
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        let dir = self.checkout_dir(ctx);
        let path = cargo_target_dir(&dir).join(self.target()).join("release").join("coreutils");
        vec![BuildOutput {
            description: "coreutils multicall binary".to_string(),
            path,
            rootfs_install: Some(RootfsInstall {
                dest: PathBuf::from("bin/coreutils"),
                symlinks: COREUTILS_APPLETS.iter().map(|a| PathBuf::from("bin").join(a)).collect(),
            }),
        }]
    }

    fn install_mode(&self) -> InstallMode {
        InstallMode::StaticArtifacts
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// uutils' multi-call dispatch determines which applet argv[0]/a symlink
/// name maps to. On non-musl Linux it distrusts argv[0] and instead reads
/// the kernel's AT_EXECFN auxval — but if AT_EXECFN comes back empty
/// (observed on this build host and inside our own built kernel), it uses
/// that empty path as the binary name instead of falling back to argv0,
/// so every applet invocation hits "<unknown binary name>". Patches in a
/// fallback to argv0 for that one case.
fn patch_binary_path(dir: &Path) -> Result<()> {
    let path = dir.join("src/common/validation.rs");
    let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;

    const ALREADY_PATCHED: &str = "execfn_bytes.is_empty()\n        || execfn_bytes.rsplit";
    if text.contains(ALREADY_PATCHED) {
        println!("uutils binary_path already patched");
        return Ok(());
    }

    const OLD: &str = "    if execfn_bytes.rsplit(|&b| b == b'/').next() == argv0.as_bytes().rsplit(|&b| b == b'/').next()\n        || execfn_bytes.starts_with(b\"/proc/\")";
    const NEW: &str = "    if execfn_bytes.is_empty()\n        || execfn_bytes.rsplit(|&b| b == b'/').next() == argv0.as_bytes().rsplit(|&b| b == b'/').next()\n        || execfn_bytes.starts_with(b\"/proc/\")";

    if !text.contains(OLD) {
        bail!(
            "couldn't find the expected binary_path code in {} to patch (uutils upstream may have changed it)",
            path.display()
        );
    }

    println!("patching uutils binary_path (AT_EXECFN-empty fallback)");
    std::fs::write(&path, text.replace(OLD, NEW)).with_context(|| format!("writing {}", path.display()))
}
