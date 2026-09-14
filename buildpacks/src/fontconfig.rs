//! fontconfig — font matching/configuration, an optional (`required:
//! false`) dependency of weston's shared cairo helper, but pulled in
//! anyway (see `Cairo`'s doc comment) since it's needed for cairo's own
//! font backend to do anything useful.

use anyhow::Context;
use buildpack_core::build::meson_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::Deserialize;
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct FontconfigConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Fontconfig {
    cfg: FontconfigConfig,
}

impl Fontconfig {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("fontconfig-{}", self.cfg.version))
    }
}

impl Buildpack for Fontconfig {
    fn id(&self) -> &'static str {
        "fontconfig"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [fontconfig] config")?;
        Ok(())
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &["freetype", "expat"]
    }

    fn describe(&self) -> Description {
        Description {
            id: "fontconfig",
            name: "fontconfig",
            summary: "Font matching/configuration — freetype's companion for cairo's font backend",
            long_description: "Meson build, docs/tests/nls disabled. XML config parsing via \
                expat, not libxml2.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("fontconfig-{}.tar.xz", self.cfg.version),
            extracted_dir_name: format!("fontconfig-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let dir = self.build_dir(ctx);
        // Sysroot-installed .pc, not the build-dir-local meson-private
        // copy — see buildpacks::weston::Weston::build's doc comment for
        // why (a false "already built" signal on a partial failed build).
        let marker = ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/fontconfig.pc");

        if already_built(&marker, force) {
            println!("skip build-fontconfig: {} already exists", marker.display());
            return Ok(());
        }

        println!("configuring/building/installing fontconfig in {}", dir.display());
        meson_build_and_install(
            ctx,
            &dir,
            &["-Ddoc=disabled", "-Dnls=disabled", "-Dtests=disabled", "-Dcache-build=disabled"],
        )
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "fontconfig.pc (sysroot marker)".to_string(),
            path: ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/fontconfig.pc"),
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
