//! libpng — weston's `dependency('libpng')` (unconditional, not optional)
//! and cairo's PNG image-surface backend. Depends on zlib.

use anyhow::Context;
use buildpack_core::build::autotools_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::Deserialize;
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct LibpngConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Libpng {
    cfg: LibpngConfig,
}

impl Libpng {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("libpng-{}", self.cfg.version))
    }
}

impl Buildpack for Libpng {
    fn id(&self) -> &'static str {
        "libpng"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [libpng] config")?;
        Ok(())
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &["zlib"]
    }

    fn describe(&self) -> Description {
        Description {
            id: "libpng",
            name: "libpng",
            summary: "PNG image library — weston and cairo both require it unconditionally",
            long_description: "Autotools build, depends on zlib.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("libpng-{}.tar.xz", self.cfg.version),
            extracted_dir_name: format!("libpng-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let dir = self.build_dir(ctx);
        let marker = dir.join(".libs").join("libpng16.a");

        if already_built(&marker, force) {
            println!("skip build-libpng: {} already exists", marker.display());
            return Ok(());
        }

        println!("configuring/building/installing libpng in {}", dir.display());
        autotools_build_and_install(ctx, &dir, &["--libdir=/usr/lib/x86_64-linux-gnu"])
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "libpng.pc (sysroot marker)",
            path: ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/libpng16.pc"),
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
