//! Software rasterization — used both as Mesa's software fallback path
//! and directly by some compositor code for operations not worth doing
//! on the GPU.

use anyhow::Context;
use buildpack_core::build::meson_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PixmanConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Pixman {
    cfg: PixmanConfig,
}

impl Pixman {
    pub fn new() -> Self {
        Self::default()
    }

    /// Same GitLab-archive double-naming as libxkbcommon's GitHub one:
    /// extracts to `pixman-pixman-<version>/`.
    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("pixman-pixman-{}", self.cfg.version))
    }
}

impl Buildpack for Pixman {
    fn id(&self) -> &'static str {
        "pixman"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [pixman] config")?;
        Ok(())
    }

    fn to_toml(&self) -> anyhow::Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [pixman] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    fn describe(&self) -> Description {
        Description {
            id: "pixman",
            name: "pixman",
            summary: "Software rasterization — Mesa's software fallback path",
            long_description: "Meson build, tests/demos/gtk/libpng/openmp all disabled.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("pixman-{}.tar.gz", self.cfg.version),
            extracted_dir_name: format!("pixman-pixman-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let marker = ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/pixman-1.pc");
        if already_built(&marker, force) {
            println!("skip build-pixman: {} already exists", marker.display());
            return Ok(());
        }

        let dir = self.build_dir(ctx);
        println!("configuring/building/installing pixman in {}", dir.display());
        meson_build_and_install(
            ctx,
            &dir,
            &["-Dtests=disabled", "-Ddemos=disabled", "-Dgtk=disabled", "-Dlibpng=disabled", "-Dopenmp=disabled"],
        )
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "pixman-1.pc (sysroot marker)".to_string(),
            path: ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/pixman-1.pc"),
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
