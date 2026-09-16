//! bash — the login shell. Built static, installed as a standalone
//! binary (no DESTDIR/sysroot install).

use anyhow::Context;
use buildpack_core::build::autotools_build_static;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, RootfsInstall, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct BashConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Bash {
    cfg: BashConfig,
}

impl Bash {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("bash-{}", self.cfg.version))
    }
}

impl Buildpack for Bash {
    fn id(&self) -> &'static str {
        "bash"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [bash] config")?;
        Ok(())
    }

    fn to_toml(&self) -> anyhow::Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [bash] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    fn required(&self) -> bool {
        true // the only shell this image has
    }

    fn describe(&self) -> Description {
        Description {
            id: "bash",
            name: "bash",
            summary: "The login shell",
            long_description: "Autotools build, statically linked (LDFLAGS=-static at \
                configure time — unlike util-linux/shadow, bash doesn't need the make-time \
                -all-static workaround).",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("bash-{}.tar.gz", self.cfg.version),
            extracted_dir_name: format!("bash-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let dir = self.build_dir(ctx);
        let marker = dir.join("bash");

        if already_built(&marker, force) {
            println!("skip build-bash: {} already exists", marker.display());
            return Ok(());
        }

        println!("configuring bash (static) in {}", dir.display());
        autotools_build_static(
            &dir,
            &["--without-bash-malloc"],
            &[("LDFLAGS", "-static")],
            &[],
        )
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "bash binary".to_string(),
            path: self.build_dir(ctx).join("bash"),
            rootfs_install: Some(RootfsInstall {
                dest: PathBuf::from("bin/bash"),
                symlinks: vec![PathBuf::from("bin/sh")],
            }),
        }]
    }

    fn install_mode(&self) -> InstallMode {
        InstallMode::StaticArtifacts
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
