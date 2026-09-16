//! fcft — foot's font-loading/rasterization/glyph-cache library (by the
//! same author, deliberately independent of pango/glib/cairo — the whole
//! reason foot is so much lighter than sway's own pango dependency).

use anyhow::Context;
use buildpack_core::build::meson_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct FcftConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Fcft {
    cfg: FcftConfig,
}

impl Fcft {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join("fcft")
    }
}

impl Buildpack for Fcft {
    fn id(&self) -> &'static str {
        "fcft"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [fcft] config")?;
        Ok(())
    }

    fn to_toml(&self) -> anyhow::Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [fcft] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &["fontconfig", "freetype", "pixman", "tllist"]
    }

    fn describe(&self) -> Description {
        Description {
            id: "fcft",
            name: "fcft",
            summary: "Font loading/rasterization library — foot's dependency",
            long_description: "Meson build. harfbuzz-based complex text shaping/grapheme \
                clustering disabled (foot's own dependency list omits utf8proc/harfbuzz), \
                nanosvg used for the bundled SVG backend (vendored, no external librsvg).",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("fcft-{}.tar.gz", self.cfg.version),
            extracted_dir_name: "fcft".to_string(),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let marker = ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/fcft.pc");
        if already_built(&marker, force) {
            println!("skip build-fcft: {} already exists", marker.display());
            return Ok(());
        }

        let dir = self.build_dir(ctx);
        println!("configuring/building/installing fcft in {}", dir.display());
        meson_build_and_install(
            ctx,
            &dir,
            &[
                "-Dgrapheme-shaping=disabled",
                "-Drun-shaping=disabled",
                "-Dsvg-backend=none",
                "-Ddocs=disabled",
                "-Dexamples=false",
            ],
        )
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "fcft.pc (sysroot marker)".to_string(),
            path: ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/fcft.pc"),
            rootfs_install: None,
        }]
    }

    fn install_mode(&self) -> InstallMode {
        InstallMode::Sysroot
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
