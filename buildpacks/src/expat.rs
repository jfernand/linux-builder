//! expat — fontconfig's XML parser dependency.

use anyhow::Context;
use buildpack_core::build::autotools_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::Deserialize;
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ExpatConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Expat {
    cfg: ExpatConfig,
}

impl Expat {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("expat-{}", self.cfg.version))
    }
}

impl Buildpack for Expat {
    fn id(&self) -> &'static str {
        "expat"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [expat] config")?;
        Ok(())
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    fn describe(&self) -> Description {
        Description {
            id: "expat",
            name: "expat",
            summary: "XML parser — fontconfig's config-file dependency",
            long_description: "Autotools build, docs/tests disabled.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("expat-{}.tar.xz", self.cfg.version),
            extracted_dir_name: format!("expat-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let dir = self.build_dir(ctx);
        // expat's library code lives under lib/, not the top level.
        let marker = dir.join("lib").join(".libs").join("libexpat.a");

        if already_built(&marker, force) {
            println!("skip build-expat: {} already exists", marker.display());
            return Ok(());
        }

        println!("configuring/building/installing expat in {}", dir.display());
        autotools_build_and_install(
            ctx,
            &dir,
            &[
                "--libdir=/usr/lib/x86_64-linux-gnu",
                "--without-docbook",
                "--disable-tests",
                "--without-examples",
            ],
        )
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "expat.pc (sysroot marker)",
            path: ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/expat.pc"),
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
