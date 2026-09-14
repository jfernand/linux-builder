//! zlib — the first link in weston's cairo dependency chain
//! (cairo/libpng need it). Custom configure script, not GNU autoconf, but
//! compatible enough with `autotools_build_and_install` (`--prefix`/
//! `DESTDIR` both supported).

use anyhow::Context;
use buildpack_core::build::autotools_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ZlibConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Zlib {
    cfg: ZlibConfig,
}

impl Zlib {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("zlib-{}", self.cfg.version))
    }
}

impl Buildpack for Zlib {
    fn id(&self) -> &'static str {
        "zlib"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [zlib] config")?;
        Ok(())
    }

    fn to_toml(&self) -> anyhow::Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [zlib] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    fn describe(&self) -> Description {
        Description {
            id: "zlib",
            name: "zlib",
            summary: "Compression library — libpng/cairo's dependency",
            long_description: "Custom configure script, not autoconf, but supports \
                --prefix/DESTDIR the same way.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("zlib-{}.tar.gz", self.cfg.version),
            extracted_dir_name: format!("zlib-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let dir = self.build_dir(ctx);
        let marker = dir.join("libz.a");

        if already_built(&marker, force) {
            println!("skip build-zlib: {} already exists", marker.display());
            return Ok(());
        }

        println!("configuring/building/installing zlib in {}", dir.display());
        // Explicit --libdir: zlib's own configure (not GNU autoconf)
        // defaults to plain lib/, not the Debian multiarch
        // lib/x86_64-linux-gnu/ that sysroot_env's PKG_CONFIG_LIBDIR
        // actually searches — same fix build_eudev already needed.
        autotools_build_and_install(ctx, &dir, &["--libdir=/usr/lib/x86_64-linux-gnu"])
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "zlib.pc (sysroot marker)".to_string(),
            path: ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/zlib.pc"),
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
