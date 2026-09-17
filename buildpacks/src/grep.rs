//! GNU grep — never part of coreutils (GNU or `uutils`), a separate
//! project entirely. `buildpacks/src/uutils.rs`'s `COREUTILS_APPLETS`
//! list wrongly included it (along with `sed`/`find`/`ps`) as if it
//! were a coreutils applet — the symlink existed but dispatched to
//! nothing, `coreutils: unknown program 'grep'`, hit repeatedly this
//! project's whole history of interactive debugging in the built image.

use anyhow::Context;
use buildpack_core::build::autotools_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct GrepConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Grep {
    cfg: GrepConfig,
}

impl Grep {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("grep-{}", self.cfg.version))
    }
}

impl Buildpack for Grep {
    fn id(&self) -> &'static str {
        "grep"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [grep] config")?;
        Ok(())
    }

    fn to_toml(&self) -> anyhow::Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [grep] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    fn describe(&self) -> Description {
        Description {
            id: "grep",
            name: "GNU grep",
            summary: "Pattern-matching text search — never part of coreutils",
            long_description: "Autotools build. NLS and PCRE (perl-regexp) support both \
                disabled — grep's own built-in basic/extended regex engine needs neither.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("grep-{}.tar.xz", self.cfg.version),
            extracted_dir_name: format!("grep-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let marker = ctx.sysroot_dir.join("usr/bin/grep");
        if already_built(&marker, force) {
            println!("skip build-grep: {} already exists", marker.display());
            return Ok(());
        }

        let dir = self.build_dir(ctx);
        println!("configuring/building/installing grep in {}", dir.display());
        autotools_build_and_install(ctx, &dir, &["--disable-nls", "--disable-perl-regexp"])
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "grep binary (sysroot marker)".to_string(),
            path: ctx.sysroot_dir.join("usr/bin/grep"),
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
