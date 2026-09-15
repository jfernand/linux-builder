//! `cosmic-bg` — COSMIC's background/wallpaper renderer, the second real
//! COSMIC component in this workspace (`cosmic-comp`, §10.3.6, is the
//! first). It's a standalone Wayland client (layer-shell background
//! surface via `smithay-client-toolkit`), not part of the session
//! orchestrator (`cosmic-session`) — same role `weston-simple-egl` played
//! for Weston's own real-boot milestone: the smallest real client that
//! proves a compositor built here can actually host something.
//!
//! Verified via a real `cargo check --no-default-features` against this
//! workspace's existing sysroot before this buildpack was written: every
//! system dependency (`wayland-client`, `libxkbcommon`, ...) is already
//! satisfied — no new C library buildpacks needed.
//!
//! `--no-default-features` drops cosmic-bg's own `avif` feature
//! (`image/avif-native`), which pulls in `dav1d-sys` — a real C library
//! (`libdav1d`, an AV1 decoder) this workspace doesn't build. Every other
//! image format cosmic-bg supports (jpeg/png/webp/hdr, plus JPEG XL via
//! the pure-Rust `jxl-oxide`) still works; only `.avif` wallpapers are
//! unsupported, an acceptable loss for a kiosk background.

use anyhow::Context;
use anyhow::{bail, Result};
use buildpack_core::build::{cargo_build_release_sysroot, cargo_target_dir};
use buildpack_core::run::already_built;
use buildpack_core::{
    BuildCtx, BuildOutput, Buildpack, Description, InstallMode, RootfsInstall, Source, SourcePatch,
};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct CosmicBgConfig {
    pub git_url: String,
    pub git_rev: String,
}

#[derive(Default)]
pub struct CosmicBg {
    cfg: CosmicBgConfig,
}

impl CosmicBg {
    pub fn new() -> Self {
        Self::default()
    }

    fn checkout_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join("cosmic-bg")
    }

    fn binary_path(&self, ctx: &BuildCtx) -> PathBuf {
        cargo_target_dir(&self.checkout_dir(ctx)).join("release").join("cosmic-bg")
    }
}

impl Buildpack for CosmicBg {
    fn id(&self) -> &'static str {
        "cosmic_bg"
    }

    fn configure(&mut self, table: &toml::Value) -> Result<()> {
        self.cfg = table.clone().try_into().context("parsing [cosmic_bg] config")?;
        Ok(())
    }

    fn to_toml(&self) -> Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [cosmic_bg] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        // Build-order only, matching every other Wayland-client buildpack
        // here (§10.3's "Dependency order" note): cosmic-bg links against
        // wayland-client/libxkbcommon, and only ever connects to
        // cosmic-comp's socket at runtime.
        &["wayland", "libxkbcommon"]
    }

    fn describe(&self) -> Description {
        Description {
            id: "cosmic_bg",
            name: "cosmic-bg",
            summary: "COSMIC's background/wallpaper renderer, a standalone Wayland client",
            long_description: "Native cargo build against this workspace's existing sysroot, \
                --no-default-features to skip the avif image codec (needs libdav1d, a C library \
                this workspace doesn't build) — every other image format still works.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Git {
            url: self.cfg.git_url.clone(),
            rev: self.cfg.git_rev.clone(),
            checkout_dir_name: "cosmic-bg".to_string(),
        }]
    }

    fn patches(&self, _ctx: &BuildCtx) -> Vec<SourcePatch> {
        vec![SourcePatch {
            description: "fall back to a solid color, not a wallpaper image this workspace doesn't ship",
            apply: patch_fallback_source,
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> Result<()> {
        let binary = self.binary_path(ctx);
        if already_built(&binary, force) {
            println!("skip build-cosmic-bg: {} already exists", binary.display());
            return Ok(());
        }

        let dir = self.checkout_dir(ctx);
        println!("building cosmic-bg in {}", dir.display());
        cargo_build_release_sysroot(
            ctx,
            &dir,
            &["--no-default-features"],
            &[
                ("RUSTUP_TOOLCHAIN", "stable"),
                ("CARGO_NET_GIT_FETCH_WITH_CLI", "true"),
            ],
        )
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "cosmic-bg binary".to_string(),
            path: self.binary_path(ctx),
            rootfs_install: Some(RootfsInstall {
                dest: PathBuf::from("usr/bin/cosmic-bg"),
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

/// `cosmic-bg-config`'s `Entry::fallback()` — used whenever no
/// `cosmic-config` state exists yet, i.e. every fresh boot of this image —
/// points at `/usr/share/backgrounds/cosmic/orion_nebula_nasa_heic0601a.jpg`,
/// part of the separate `cosmic-backgrounds` data package this project
/// doesn't build or ship. Left as-is, cosmic-bg would just find no image
/// at that path and quietly render nothing — a real regression from "no
/// cosmic-bg at all" only in that it wastes a whole extra service for
/// zero visible effect. Patches the fallback to a solid color instead,
/// so a fresh boot has something real to show.
fn patch_fallback_source(dir: &std::path::Path) -> Result<()> {
    let path = dir.join("config/src/lib.rs");
    let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;

    const ALREADY_PATCHED: &str = "source: Source::Color(Color::Single(";
    if text.contains(ALREADY_PATCHED) {
        println!("cosmic-bg-config fallback already patched");
        return Ok(());
    }

    const OLD: &str = "source: Source::Path(PathBuf::from(\n                \
        \"/usr/share/backgrounds/cosmic/orion_nebula_nasa_heic0601a.jpg\",\n            \
        )),";
    const NEW: &str = "source: Source::Color(Color::Single([0.05, 0.08, 0.16])),";

    if !text.contains(OLD) {
        bail!(
            "couldn't find the expected Entry::fallback() source text in {} \
             (upstream may have changed it)",
            path.display()
        );
    }

    println!("patching cosmic-bg-config: Entry::fallback() uses a solid color, not a missing wallpaper asset");
    let text = text.replace(OLD, NEW);
    std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))
}
