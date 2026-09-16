//! HarfBuzz — pango's text-shaping engine dependency.

use anyhow::Context;
use buildpack_core::build::meson_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct HarfbuzzConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Harfbuzz {
    cfg: HarfbuzzConfig,
}

impl Harfbuzz {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("harfbuzz-{}", self.cfg.version))
    }
}

impl Buildpack for Harfbuzz {
    fn id(&self) -> &'static str {
        "harfbuzz"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [harfbuzz] config")?;
        Ok(())
    }

    fn to_toml(&self) -> anyhow::Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [harfbuzz] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &["freetype"]
    }

    fn describe(&self) -> Description {
        Description {
            id: "harfbuzz",
            name: "HarfBuzz",
            summary: "Text-shaping engine — pango's dependency",
            long_description: "Meson build. glib/gobject/cairo/chafa/icu integrations all \
                disabled (pango only needs core harfbuzz + freetype support); tests/\
                introspection/docs/utilities/benchmark all disabled too.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("harfbuzz-{}.tar.xz", self.cfg.version),
            extracted_dir_name: format!("harfbuzz-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let marker = ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/harfbuzz.pc");
        if already_built(&marker, force) {
            println!("skip build-harfbuzz: {} already exists", marker.display());
            return Ok(());
        }

        let dir = self.build_dir(ctx);
        println!("configuring/building/installing harfbuzz in {}", dir.display());
        meson_build_and_install(
            ctx,
            &dir,
            &[
                "-Dfreetype=enabled",
                "-Dglib=disabled",
                "-Dgobject=disabled",
                "-Dcairo=disabled",
                "-Dchafa=disabled",
                "-Dicu=disabled",
                "-Dtests=disabled",
                "-Dintrospection=disabled",
                "-Ddocs=disabled",
                "-Dutilities=disabled",
                "-Dbenchmark=disabled",
            ],
        )
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "harfbuzz.pc (sysroot marker)".to_string(),
            path: ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/harfbuzz.pc"),
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
