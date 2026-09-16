//! PCRE2 — sway's config-file regex engine dependency (`libpcre2-8`).

use anyhow::Context;
use buildpack_core::build::autotools_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Pcre2Config {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Pcre2 {
    cfg: Pcre2Config,
}

impl Pcre2 {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("pcre2-{}", self.cfg.version))
    }
}

impl Buildpack for Pcre2 {
    fn id(&self) -> &'static str {
        "pcre2"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [pcre2] config")?;
        Ok(())
    }

    fn to_toml(&self) -> anyhow::Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [pcre2] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    fn describe(&self) -> Description {
        Description {
            id: "pcre2",
            name: "PCRE2",
            summary: "Regex engine — sway's config-file dependency (libpcre2-8), also glib's",
            long_description: "Autotools build, docs/tests/the standalone pcre2grep/pcre2test \
                tools all disabled — just libpcre2-8.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("pcre2-{}.tar.bz2", self.cfg.version),
            extracted_dir_name: format!("pcre2-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let marker = ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/libpcre2-8.pc");
        if already_built(&marker, force) {
            println!("skip build-pcre2: {} already exists", marker.display());
            return Ok(());
        }

        let dir = self.build_dir(ctx);
        println!("configuring/building/installing pcre2 in {}", dir.display());
        autotools_build_and_install(
            ctx,
            &dir,
            &[
                "--libdir=/usr/lib/x86_64-linux-gnu",
                "--disable-static",
                "--enable-shared",
                "--disable-pcre2grep-libz",
                "--disable-pcre2grep-libbz2",
                "--disable-pcre2test-libedit",
                "--disable-pcre2test-libreadline",
            ],
        )
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "libpcre2-8.pc (sysroot marker)".to_string(),
            path: ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/libpcre2-8.pc"),
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
