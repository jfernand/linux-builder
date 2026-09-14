//! FreeType — font rasterization, needed by cairo's toy-font API and by
//! fontconfig. harfbuzz/bzip2/brotli/png are all explicitly disabled —
//! none of them are buildpacks (harfbuzz especially: it has its own
//! optional freetype dependency, so leaving it undisabled risks a
//! circular-looking probe even though neither side hard-requires it).

use anyhow::Context;
use buildpack_core::build::autotools_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::Deserialize;
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct FreetypeConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Freetype {
    cfg: FreetypeConfig,
}

impl Freetype {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("freetype-{}", self.cfg.version))
    }
}

impl Buildpack for Freetype {
    fn id(&self) -> &'static str {
        "freetype"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [freetype] config")?;
        Ok(())
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &["zlib"]
    }

    fn describe(&self) -> Description {
        Description {
            id: "freetype",
            name: "FreeType",
            summary: "Font rasterization — cairo/fontconfig's dependency",
            long_description: "Autotools build. harfbuzz/bzip2/brotli/png support all \
                disabled — none are buildpacks, and none are needed for cairo's \
                toy-font API.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("freetype-{}.tar.xz", self.cfg.version),
            extracted_dir_name: format!("freetype-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let dir = self.build_dir(ctx);
        let marker = dir.join("objs").join(".libs").join("libfreetype.a");

        if already_built(&marker, force) {
            println!("skip build-freetype: {} already exists", marker.display());
            return Ok(());
        }

        println!("configuring/building/installing freetype in {}", dir.display());
        autotools_build_and_install(
            ctx,
            &dir,
            &[
                "--libdir=/usr/lib/x86_64-linux-gnu",
                "--with-harfbuzz=no",
                "--with-bzip2=no",
                "--with-png=no",
                "--with-brotli=no",
            ],
        )
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "freetype2.pc (sysroot marker)".to_string(),
            path: ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/freetype2.pc"),
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
