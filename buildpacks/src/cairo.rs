//! cairo — the package that blocked weston all session: `shared/meson.build`
//! builds `dependency('cairo')` and `dependency('libpng')` unconditionally
//! (no `required: false`), and `libweston`'s GL-renderer window-border code
//! depends on it too — not avoidable via a meson option, not just an
//! optional demo-client dependency. X11/xcb/GL backends all disabled (no
//! libxcb buildpack, and weston only needs cairo's image-surface backend).

use anyhow::Context;
use buildpack_core::build::meson_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct CairoConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Cairo {
    cfg: CairoConfig,
}

impl Cairo {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("cairo-{}", self.cfg.version))
    }
}

impl Buildpack for Cairo {
    fn id(&self) -> &'static str {
        "cairo"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [cairo] config")?;
        Ok(())
    }

    fn to_toml(&self) -> anyhow::Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [cairo] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &["freetype", "fontconfig", "libpng", "zlib", "pixman"]
    }

    fn describe(&self) -> Description {
        Description {
            id: "cairo",
            name: "cairo",
            summary: "2D graphics library — weston's mandatory window-decoration dependency",
            long_description: "Meson build. X11/xcb/GL backends disabled (image-surface \
                backend only, no libxcb buildpack); tests/docs disabled.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("cairo-{}.tar.xz", self.cfg.version),
            extracted_dir_name: format!("cairo-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let dir = self.build_dir(ctx);
        // Sysroot-installed .pc, not the build-dir-local meson-private
        // copy — see Weston::build's doc comment for why (that same
        // false-"already built" signal on a partial failed build was
        // actually hit and fixed there first).
        let marker = ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/cairo.pc");

        if already_built(&marker, force) {
            println!("skip build-cairo: {} already exists", marker.display());
            return Ok(());
        }

        println!("configuring/building/installing cairo in {}", dir.display());
        meson_build_and_install(
            ctx,
            &dir,
            &[
                "-Dtests=disabled",
                "-Dfontconfig=enabled",
                "-Dfreetype=enabled",
                "-Dzlib=enabled",
                "-Dpng=enabled",
                "-Dxlib=disabled",
                "-Dxcb=disabled",
                "-Dglib=disabled",
                "-Dgtk_doc=false",
                "-Dspectre=disabled",
                "-Dsymbol-lookup=disabled",
            ],
        )
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "cairo.pc (sysroot marker)".to_string(),
            path: ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/cairo.pc"),
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
