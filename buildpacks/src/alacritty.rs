//! Alacritty — a real, non-COSMIC terminal emulator, picked over
//! `cosmic-term` for being dramatically lighter (a plain `winit`+`glutin`
//! Wayland client, no `libcosmic`/`iced` dependency chain) while still
//! satisfying the Rust/C-only rule. Ghostty was considered and rejected:
//! its core is Zig, not Rust or C.
//!
//! Verified via a real `cargo check --release -p alacritty
//! --no-default-features --features wayland` against this workspace's
//! existing sysroot before this buildpack was written: every system
//! dependency is already satisfied — no new C library buildpacks needed.
//! `--no-default-features --features wayland` drops the `x11` feature
//! (`x11-dl`, `glutin/glx`, `png`) — this workspace has no X11, matching
//! the Wayland-only policy every other package here already follows.

use anyhow::Context;
use anyhow::Result;
use buildpack_core::build::{cargo_build_release_sysroot, cargo_target_dir};
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, RootfsInstall, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AlacrittyConfig {
    pub git_url: String,
    pub git_rev: String,
}

#[derive(Default)]
pub struct Alacritty {
    cfg: AlacrittyConfig,
}

impl Alacritty {
    pub fn new() -> Self {
        Self::default()
    }

    fn checkout_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join("alacritty")
    }

    fn binary_path(&self, ctx: &BuildCtx) -> PathBuf {
        cargo_target_dir(&self.checkout_dir(ctx)).join("release").join("alacritty")
    }
}

impl Buildpack for Alacritty {
    fn id(&self) -> &'static str {
        "alacritty"
    }

    fn configure(&mut self, table: &toml::Value) -> Result<()> {
        self.cfg = table.clone().try_into().context("parsing [alacritty] config")?;
        Ok(())
    }

    fn to_toml(&self) -> Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [alacritty] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        // Build-order only, same as every other Wayland-client buildpack
        // here — links against wayland-client/libxkbcommon, never touches
        // any C library at build time beyond those.
        &["wayland", "libxkbcommon"]
    }

    fn functional_dependencies(&self) -> &'static [&'static str] {
        // Confirmed via a real QEMU boot: without an actual font file
        // present, Alacritty fails outright at startup with
        // Font(FontNotFound(...)) — fontconfig (the library) has nothing
        // to resolve "monospace" to without it.
        &["dejavu_fonts"]
    }

    fn describe(&self) -> Description {
        Description {
            id: "alacritty",
            name: "Alacritty",
            summary: "A real, lightweight terminal emulator (not a COSMIC component)",
            long_description: "Native cargo build against this workspace's existing sysroot, \
                --no-default-features --features wayland to drop X11 support (this workspace has \
                none). Picked over cosmic-term for being far lighter, and over Ghostty for being \
                Rust rather than Zig.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Git {
            url: self.cfg.git_url.clone(),
            rev: self.cfg.git_rev.clone(),
            checkout_dir_name: "alacritty".to_string(),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> Result<()> {
        let binary = self.binary_path(ctx);
        if already_built(&binary, force) {
            println!("skip build-alacritty: {} already exists", binary.display());
            return Ok(());
        }

        let dir = self.checkout_dir(ctx);
        println!("building alacritty in {}", dir.display());
        cargo_build_release_sysroot(
            ctx,
            &dir,
            &["-p", "alacritty", "--no-default-features", "--features", "wayland"],
            &[
                ("RUSTUP_TOOLCHAIN", "stable"),
                ("CARGO_NET_GIT_FETCH_WITH_CLI", "true"),
            ],
        )
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "alacritty binary".to_string(),
            path: self.binary_path(ctx),
            rootfs_install: Some(RootfsInstall {
                dest: PathBuf::from("usr/bin/alacritty"),
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
