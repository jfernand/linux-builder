//! shadow-utils — `login`/`passwd`. Static, no PAM/SELinux (nothing in
//! this rootfs needs them).

use anyhow::Context;
use buildpack_core::build::autotools_build_static;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, RootfsInstall, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ShadowConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Shadow {
    cfg: ShadowConfig,
}

impl Shadow {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("shadow-{}", self.cfg.version))
    }
}

impl Buildpack for Shadow {
    fn id(&self) -> &'static str {
        "shadow"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [shadow] config")?;
        Ok(())
    }

    fn to_toml(&self) -> anyhow::Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [shadow] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    fn required(&self) -> bool {
        true // login/passwd — nothing to authenticate against without it
    }

    fn describe(&self) -> Description {
        Description {
            id: "shadow",
            name: "shadow-utils",
            summary: "login/passwd — real PAM-free authentication",
            long_description: "Autotools build, statically linked (make-time \
                LDFLAGS=-all-static, same libtool workaround as util-linux).",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("shadow-{}.tar.xz", self.cfg.version),
            extracted_dir_name: format!("shadow-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let dir = self.build_dir(ctx);
        let marker = dir.join("src").join("login");

        if already_built(&marker, force) {
            println!("skip build-shadow: {} already exists", marker.display());
            return Ok(());
        }

        println!("configuring shadow-utils (static, no PAM/SELinux) in {}", dir.display());
        autotools_build_static(
            &dir,
            &[
                "--without-libpam",
                "--without-selinux",
                "--without-acl",
                "--without-attr",
                "--without-audit",
                "--disable-nls",
                "--disable-account-tools-setuid",
                "--disable-shared",
                "--enable-static",
            ],
            &[],
            &[("LDFLAGS", "-all-static")],
        )
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        let dir = self.build_dir(ctx);
        vec![
            BuildOutput {
                description: "login binary".to_string(),
                path: dir.join("src").join("login"),
                rootfs_install: Some(RootfsInstall {
                    dest: PathBuf::from("bin/login"),
                    symlinks: vec![],
                }),
            },
            BuildOutput {
                description: "passwd binary".to_string(),
                path: dir.join("src").join("passwd"),
                rootfs_install: Some(RootfsInstall {
                    dest: PathBuf::from("bin/passwd"),
                    symlinks: vec![],
                }),
            },
        ]
    }

    fn install_mode(&self) -> InstallMode {
        InstallMode::StaticArtifacts
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
