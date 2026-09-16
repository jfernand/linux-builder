//! Pango — sway's text-layout/rendering dependency, required unconditionally
//! (not gated by the `swaybar`/`swaynag` meson options) since sway 1.12's
//! own `meson.build` declares `pango`/`pangocairo` as plain `dependency()`
//! calls with no `required: get_option(...)` guard.

use anyhow::Context;
use buildpack_core::build::meson_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PangoConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Pango {
    cfg: PangoConfig,
}

impl Pango {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("pango-{}", self.cfg.version))
    }
}

impl Buildpack for Pango {
    fn id(&self) -> &'static str {
        "pango"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [pango] config")?;
        Ok(())
    }

    fn to_toml(&self) -> anyhow::Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [pango] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &["glib", "harfbuzz", "fribidi", "cairo", "fontconfig", "freetype"]
    }

    fn describe(&self) -> Description {
        Description {
            id: "pango",
            name: "Pango",
            summary: "Text layout/rendering — sway's unconditional swaybar/swaynag dependency",
            long_description: "Meson build. X11/xft/quartz backends, introspection, \
                documentation, and the test/example suites all disabled — just the \
                glib+cairo+fontconfig+freetype+harfbuzz+fribidi text-layout core sway links.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("pango-{}.tar.xz", self.cfg.version),
            extracted_dir_name: format!("pango-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let marker = ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/pangocairo.pc");
        if already_built(&marker, force) {
            println!("skip build-pango: {} already exists", marker.display());
            return Ok(());
        }

        let dir = self.build_dir(ctx);
        println!("configuring/building/installing pango in {}", dir.display());
        meson_build_and_install(
            ctx,
            &dir,
            &[
                "-Dcairo=enabled",
                "-Dfontconfig=enabled",
                "-Dfreetype=enabled",
                "-Dxft=disabled",
                "-Dlibthai=disabled",
                "-Dsysprof=disabled",
                "-Dintrospection=disabled",
                "-Ddocumentation=false",
                "-Dgtk_doc=false",
                "-Dbuild-testsuite=false",
                "-Dbuild-examples=false",
            ],
        )
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "pangocairo.pc (sysroot marker)".to_string(),
            path: ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/pangocairo.pc"),
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
