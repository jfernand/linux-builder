//! GNU findutils (`find`, `xargs`, `locate`) — like `grep`/`sed`, never
//! part of coreutils; see `grep.rs`'s doc comment for the full story.

use anyhow::Context;
use buildpack_core::build::autotools_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct FindutilsConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Findutils {
    cfg: FindutilsConfig,
}

impl Findutils {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("findutils-{}", self.cfg.version))
    }
}

impl Buildpack for Findutils {
    fn id(&self) -> &'static str {
        "findutils"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [findutils] config")?;
        Ok(())
    }

    fn to_toml(&self) -> anyhow::Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [findutils] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    fn describe(&self) -> Description {
        Description {
            id: "findutils",
            name: "GNU findutils",
            summary: "find, xargs, locate — never part of coreutils",
            long_description: "Autotools build. NLS and SELinux support both disabled.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("findutils-{}.tar.xz", self.cfg.version),
            extracted_dir_name: format!("findutils-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let marker = ctx.sysroot_dir.join("usr/bin/find");
        if already_built(&marker, force) {
            println!("skip build-findutils: {} already exists", marker.display());
            return Ok(());
        }

        let dir = self.build_dir(ctx);
        println!("configuring/building/installing findutils in {}", dir.display());
        autotools_build_and_install(ctx, &dir, &["--disable-nls", "--without-selinux"])
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![
            BuildOutput {
                description: "find binary (sysroot marker)".to_string(),
                path: ctx.sysroot_dir.join("usr/bin/find"),
                rootfs_install: None,
            },
            BuildOutput {
                description: "xargs binary".to_string(),
                path: ctx.sysroot_dir.join("usr/bin/xargs"),
                rootfs_install: None,
            },
        ]
    }

    fn install_mode(&self) -> InstallMode {
        InstallMode::Sysroot
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
