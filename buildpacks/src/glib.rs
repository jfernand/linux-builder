//! GLib — pango's core dependency (glib-2.0/gobject-2.0/gio-2.0). Only
//! pulled in transitively for sway's terminal-text rendering (pango);
//! nothing else in this workspace needs it. `gvdb` (its one always-on
//! subproject dependency) ships fully vendored in GNOME's own release
//! tarball, so this builds offline like everything else here.

use anyhow::Context;
use buildpack_core::build::meson_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct GlibConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Glib {
    cfg: GlibConfig,
}

impl Glib {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("glib-{}", self.cfg.version))
    }
}

impl Buildpack for Glib {
    fn id(&self) -> &'static str {
        "glib"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [glib] config")?;
        Ok(())
    }

    fn to_toml(&self) -> anyhow::Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [glib] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &["pcre2", "zlib"]
    }

    fn describe(&self) -> Description {
        Description {
            id: "glib",
            name: "GLib",
            summary: "Core utility library — pulled in by pango, for sway's bar/nag text rendering",
            long_description: "Meson build. selinux/libmount/introspection/documentation/tests/\
                man-pages/nls/libelf all disabled — only the core glib-2.0/gobject-2.0/gio-2.0 \
                pango actually links against.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("glib-{}.tar.xz", self.cfg.version),
            extracted_dir_name: format!("glib-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let marker = ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/glib-2.0.pc");
        if already_built(&marker, force) {
            println!("skip build-glib: {} already exists", marker.display());
            return Ok(());
        }

        let dir = self.build_dir(ctx);
        println!("configuring/building/installing glib in {}", dir.display());
        meson_build_and_install(
            ctx,
            &dir,
            &[
                "-Dselinux=disabled",
                "-Dxattr=false",
                "-Dlibmount=disabled",
                "-Dtests=false",
                "-Dinstalled_tests=false",
                "-Dnls=disabled",
                "-Dlibelf=disabled",
                "-Dintrospection=disabled",
                "-Ddocumentation=false",
                "-Dman-pages=disabled",
                "-Dsysprof=disabled",
                "-Ddtrace=false",
                "-Dsystemtap=false",
                "-Dmultiarch=false",
            ],
        )
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "glib-2.0.pc (sysroot marker)".to_string(),
            path: ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/glib-2.0.pc"),
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
