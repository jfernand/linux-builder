//! GNU sed — like `grep`, never part of coreutils; see `grep.rs`'s doc
//! comment for the full story of how this project's `uutils` buildpack
//! ended up with a dead `sed` symlink.

use anyhow::Context;
use buildpack_core::build::autotools_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SedConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Sed {
    cfg: SedConfig,
}

impl Sed {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("sed-{}", self.cfg.version))
    }
}

impl Buildpack for Sed {
    fn id(&self) -> &'static str {
        "sed"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [sed] config")?;
        Ok(())
    }

    fn to_toml(&self) -> anyhow::Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [sed] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    fn describe(&self) -> Description {
        Description {
            id: "sed",
            name: "GNU sed",
            summary: "Stream editor — never part of coreutils",
            long_description: "Autotools build. NLS, ACL, and SELinux support all disabled — \
                none needed for basic stream editing (disabling SELinux support also drops \
                an unexpected transitive libpcre2-8 link gnulib pulled in alongside it).",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("sed-{}.tar.xz", self.cfg.version),
            extracted_dir_name: format!("sed-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let marker = ctx.sysroot_dir.join("usr/bin/sed");
        if already_built(&marker, force) {
            println!("skip build-sed: {} already exists", marker.display());
            return Ok(());
        }

        let dir = self.build_dir(ctx);
        println!("configuring/building/installing sed in {}", dir.display());
        autotools_build_and_install(ctx, &dir, &["--disable-nls", "--disable-acl", "--without-selinux"])
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "sed binary (sysroot marker)".to_string(),
            path: ctx.sysroot_dir.join("usr/bin/sed"),
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
