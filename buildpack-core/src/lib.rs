//! The `Buildpack` trait: a single self-contained definition of a source
//! package's source, dependencies, build instructions, and build outputs,
//! plus a TUI/CLI-facing description. See the repo's plan file
//! (`i-kinda-want-to-cozy-rainbow.md`, "Buildpack architecture: turn the
//! pipeline inside out") for the rationale.

pub mod build;
pub mod config;
pub mod graph;
pub mod install;
pub mod pipeline;
pub mod run;

use anyhow::Result;
use std::any::Any;
use std::path::{Path, PathBuf};

/// Where a buildpack's source comes from.
pub enum Source {
    /// Tarball fetched over HTTP, extracted under `sources_dir`.
    /// `extracted_dir_name` is resolved by the impl, not derived from a
    /// template — some projects' archives don't extract to `<name>-<version>`
    /// (GitHub/GitLab double-prefix naming, or libdrm's literal
    /// commit-SHA-suffixed directory name, which can't be derived from the
    /// version at all).
    Tarball { url: String, archive_name: String, extracted_dir_name: String },
    /// Git checkout (uutils today).
    Git { url: String, rev: String, checkout_dir_name: String },
    /// No independent source of its own (e.g. a future in-tree buildpack).
    /// None of today's packages need this; kept so the enum stays honest.
    InTree,
}

/// One optional post-extraction source patch. `apply` must be idempotent —
/// each impl does its own "already patched" marker-text check internally,
/// the same way `patch_uutils_binary_path`/`patch_libdisplay_info_hwdata`
/// do today.
pub struct SourcePatch {
    pub description: &'static str,
    pub apply: fn(&Path) -> Result<()>,
}

/// Where and how a buildpack's build output ends up.
pub enum InstallMode {
    /// Build produces standalone files the rootfs stage must explicitly
    /// copy in (uutils, bash, util-linux, shadow, busybox).
    StaticArtifacts,
    /// Build system installs directly into the shared sysroot via DESTDIR
    /// (meson/autotools packages from seatd onward); the rootfs stage
    /// bulk-copies the whole sysroot once, after every Sysroot-mode
    /// buildpack has built — not installed individually.
    Sysroot,
}

/// Where a `StaticArtifacts` output should land in the rootfs.
pub struct RootfsInstall {
    pub dest: PathBuf,
    pub symlinks: Vec<PathBuf>,
}

/// One concrete file/binary/library a buildpack produces — used both for
/// rootfs installation (`StaticArtifacts`) and for presence/"is this
/// built" checks (both install modes), replacing the ad hoc
/// `*_binary_path()` free functions and marker-path convention with one
/// declared, inspectable list.
pub struct BuildOutput {
    pub description: String,
    pub path: PathBuf,
    pub rootfs_install: Option<RootfsInstall>,
}

/// TUI/CLI-facing description, deliberately separate from `Source`/
/// `BuildOutput` so rendering never has to reach into build internals.
pub struct Description {
    pub id: &'static str,
    pub name: &'static str,
    pub summary: &'static str,
    pub long_description: &'static str,
}

/// Generic pipeline paths every buildpack needs. Deliberately NOT the
/// whole distro `Config` — a buildpack only ever needs these plus its own
/// already-`configure()`d fields, never another buildpack's config.
/// `image`/`kernel_bzimage` are only read by the `PipelineStage` impls
/// (`pipeline` module) — every `Buildpack` ignores them, the same way
/// static-artifact packages already ignore `sysroot_dir`.
#[derive(Clone)]
pub struct BuildCtx {
    pub sources_dir: PathBuf,
    pub build_dir: PathBuf,
    pub sysroot_dir: PathBuf,
    pub rootfs_dir: PathBuf,
    pub arch: String,
    pub networking: bool,
    pub jobs: usize,
    pub image: config::ImageSettings,
    /// The kernel buildpack's bzImage output path — `make_image` embeds
    /// it, nothing else reads it. Populated by the caller (it already has
    /// to construct the kernel's own `BuildCtx`/outputs to build it).
    pub kernel_bzimage: PathBuf,
}

pub trait Buildpack {
    /// Stable identity: the TOML config key, the dependency-graph node id.
    fn id(&self) -> &'static str;

    /// Apply this buildpack's slice of the workspace TOML (the `[<id>]`
    /// table, or an empty table if absent — packages with no config
    /// simply ignore it). Called once, right after construction.
    fn configure(&mut self, table: &toml::Value) -> Result<()>;

    /// The inverse of `configure`: reassemble this buildpack's `[<id>]`
    /// table, so `DistroConfig::save` can round-trip whatever
    /// `configure`/direct field mutation (e.g. `resolve_kernel`) changed.
    fn to_toml(&self) -> Result<toml::Value>;

    /// Declared prerequisite buildpack ids. Static and config-independent
    /// — dependencies don't change based on TOML overrides in this
    /// codebase (e.g. Mesa always depends on libdrm/wayland/libxkbcommon/
    /// pixman/libdisplay-info/libinput).
    fn dependencies(&self) -> &'static [&'static str];

    fn describe(&self) -> Description;

    /// Where this buildpack's source lives / how to get it.
    fn sources(&self, ctx: &BuildCtx) -> Vec<Source>;

    /// Post-extraction source patches, if any (empty for most).
    fn patches(&self, _ctx: &BuildCtx) -> Vec<SourcePatch> {
        Vec::new()
    }

    /// Download/extract/clone + apply patches. Default handles the common
    /// Tarball/Git cases generically via `sources()`/`patches()`; override
    /// only for unusual fetch needs.
    fn fetch(&self, ctx: &BuildCtx, force: bool) -> Result<()> {
        build::default_fetch(self, ctx, force)
    }

    /// The actual compile step. Build-system diversity (Cargo vs
    /// autotools vs meson) and per-package quirks live here, using the
    /// shared helpers in `build` (`meson_build_and_install`,
    /// `autotools_build_and_install`, `cargo_build_release`).
    fn build(&self, ctx: &BuildCtx, force: bool) -> Result<()>;

    /// This buildpack's build products — used for presence-checking and,
    /// for `StaticArtifacts` mode, by the rootfs assembler.
    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput>;

    fn install_mode(&self) -> InstallMode;

    /// Is this buildpack already built? Default: every `outputs()` path
    /// exists.
    fn is_built(&self, ctx: &BuildCtx) -> bool {
        let outs = self.outputs(ctx);
        !outs.is_empty() && outs.iter().all(|o| o.path.exists())
    }

    /// rm -rf this buildpack's build/output artifacts. Default: remove
    /// every `outputs()` path (files only — a source-dir-wide clean is
    /// left to callers, since not every buildpack's outputs live under a
    /// single removable directory, e.g. `StaticArtifacts` binaries are
    /// copied into `sources_dir`, `Sysroot` binaries into `sysroot_dir`).
    fn clean(&self, ctx: &BuildCtx) -> Result<()> {
        for out in self.outputs(ctx) {
            if out.path.exists() {
                std::fs::remove_file(&out.path)?;
            }
        }
        Ok(())
    }

    /// A narrow, deliberate escape hatch: lets a generic TUI/CLI downcast
    /// to a concrete buildpack type for the rare case where a package has
    /// its own sub-structure worth rendering specially (today: only
    /// `Kernel`'s `FeaturePack` list). Not a general pattern — most
    /// callers never use this.
    fn as_any(&self) -> &dyn Any;
}
