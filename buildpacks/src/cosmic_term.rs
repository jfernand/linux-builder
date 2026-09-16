//! `cosmic-term` — COSMIC's own terminal, built on `libcosmic`/`iced`
//! (a much heavier dependency chain than Alacritty's plain
//! `winit`+`glutin`, but the real COSMIC component §11 actually lists).
//! Reuses `alacritty_terminal` itself for PTY/terminal-state handling —
//! same backend Alacritty uses, just with COSMIC's own UI on top — and
//! renders via `wgpu` rather than raw EGL/glutin.
//!
//! Verified via a real `cargo check --release` against this workspace's
//! existing sysroot before this buildpack was written: every dependency
//! resolves already — no new C library buildpacks needed, same as every
//! other COSMIC component here. Notably it pulls in a pop-OS fork of
//! `winit` (`github.com/pop-os/winit`, tag `cosmic-0.14`), not upstream
//! `winit` — possibly relevant to Alacritty's still-unresolved upstream
//! `winit` event-loop bug against `cosmic-comp` (§10.3.6.5), though that
//! hasn't been tested here yet.

use anyhow::Context;
use anyhow::Result;
use buildpack_core::build::{cargo_build_release_sysroot, cargo_target_dir};
use buildpack_core::run::already_built;
use buildpack_core::{
    BuildCtx, BuildOutput, Buildpack, Description, InstallMode, RootfsInstall, Source, SourcePatch,
};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct CosmicTermConfig {
    pub git_url: String,
    pub git_rev: String,
}

#[derive(Default)]
pub struct CosmicTerm {
    cfg: CosmicTermConfig,
}

impl CosmicTerm {
    pub fn new() -> Self {
        Self::default()
    }

    fn checkout_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join("cosmic-term")
    }

    fn binary_path(&self, ctx: &BuildCtx) -> PathBuf {
        cargo_target_dir(&self.checkout_dir(ctx)).join("release").join("cosmic-term")
    }
}

impl Buildpack for CosmicTerm {
    fn id(&self) -> &'static str {
        "cosmic_term"
    }

    fn configure(&mut self, table: &toml::Value) -> Result<()> {
        self.cfg = table.clone().try_into().context("parsing [cosmic_term] config")?;
        Ok(())
    }

    fn to_toml(&self) -> Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [cosmic_term] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        // Build-order only, same as every other Wayland-client buildpack
        // here — links against wayland-client/libxkbcommon; the Vulkan
        // wgpu likely uses at runtime is dlopen'd (ash-style), same as
        // cosmic-comp's own Vulkan story, not a build-time link.
        &["wayland", "libxkbcommon"]
    }

    fn functional_dependencies(&self) -> &'static [&'static str] {
        // cosmic_comp: inert without a compositor to connect to, same as
        // every other client here. dejavu_fonts: cosmic-text (its own
        // text-rendering stack, separate from crossfont/fontconfig) very
        // likely hits the exact same "no font file anywhere" gap
        // Alacritty did (§10.3.6.5) — not yet independently confirmed
        // for cosmic-term specifically, but the same class of gap on the
        // same underlying fontconfig-less sysroot.
        &["cosmic_comp", "dejavu_fonts"]
    }

    fn describe(&self) -> Description {
        Description {
            id: "cosmic_term",
            name: "cosmic-term",
            summary: "COSMIC's own terminal, built on libcosmic/iced and wgpu",
            long_description: "Native cargo build against this workspace's existing sysroot — no \
                new C library dependencies. Reuses alacritty_terminal for PTY/terminal-state \
                handling, renders via wgpu, and depends on a pop-OS fork of winit rather than \
                upstream winit (unlike the Alacritty buildpack).",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Git {
            url: self.cfg.git_url.clone(),
            rev: self.cfg.git_rev.clone(),
            checkout_dir_name: "cosmic-term".to_string(),
        }]
    }

    fn patches(&self, _ctx: &BuildCtx) -> Vec<SourcePatch> {
        vec![SourcePatch {
            description: "add an empty [workspace] table so cargo stops walking up into this repo's own workspace",
            apply: patch_workspace_table,
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> Result<()> {
        let binary = self.binary_path(ctx);
        if already_built(&binary, force) {
            println!("skip build-cosmic-term: {} already exists", binary.display());
            return Ok(());
        }

        let dir = self.checkout_dir(ctx);
        println!("building cosmic-term in {}", dir.display());
        cargo_build_release_sysroot(
            ctx,
            &dir,
            &[],
            &[
                ("RUSTUP_TOOLCHAIN", "stable"),
                ("CARGO_NET_GIT_FETCH_WITH_CLI", "true"),
            ],
        )
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "cosmic-term binary".to_string(),
            path: self.binary_path(ctx),
            rootfs_install: Some(RootfsInstall {
                dest: PathBuf::from("usr/bin/cosmic-term"),
                symlinks: Vec::new(),
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

/// Unlike `cosmic-comp`/`cosmic-bg`, `cosmic-term`'s own `Cargo.toml` has
/// no `[workspace]` table — since its checkout dir lives under this
/// repo's own `build-distro/sources/`, cargo's normal upward workspace
/// search finds *this project's own* root `Cargo.toml` instead and
/// refuses to build ("current package believes it's in a workspace when
/// it's not"), exactly the fix cargo's own error message suggests.
fn patch_workspace_table(dir: &std::path::Path) -> Result<()> {
    let path = dir.join("Cargo.toml");
    let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;

    if text.contains("[workspace]") {
        println!("cosmic-term Cargo.toml already patched");
        return Ok(());
    }

    println!("patching cosmic-term Cargo.toml: adding an empty [workspace] table");
    let text = format!("{text}\n[workspace]\n");
    std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))
}
