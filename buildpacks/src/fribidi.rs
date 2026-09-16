//! FriBidi — pango's bidirectional-text (Unicode BiDi algorithm) dependency.

use anyhow::Context;
use buildpack_core::build::meson_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct FribidiConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Fribidi {
    cfg: FribidiConfig,
}

impl Fribidi {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("fribidi-{}", self.cfg.version))
    }
}

impl Buildpack for Fribidi {
    fn id(&self) -> &'static str {
        "fribidi"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [fribidi] config")?;
        Ok(())
    }

    fn to_toml(&self) -> anyhow::Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [fribidi] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    fn describe(&self) -> Description {
        Description {
            id: "fribidi",
            name: "FriBidi",
            summary: "Unicode Bidirectional Algorithm implementation — pango's dependency",
            long_description: "Meson build, docs/tests/the standalone fribidi CLI tool disabled.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("fribidi-{}.tar.xz", self.cfg.version),
            extracted_dir_name: format!("fribidi-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let marker = ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/fribidi.pc");
        if already_built(&marker, force) {
            println!("skip build-fribidi: {} already exists", marker.display());
            return Ok(());
        }

        let dir = self.build_dir(ctx);
        println!("configuring/building/installing fribidi in {}", dir.display());
        meson_build_and_install(ctx, &dir, &["-Dbin=false", "-Ddocs=false", "-Dtests=false"])
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "fribidi.pc (sysroot marker)".to_string(),
            path: ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/fribidi.pc"),
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
