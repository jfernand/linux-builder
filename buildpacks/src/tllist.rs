//! tllist — a tiny header-only typed-linked-list library, fcft/foot's
//! only non-system dependency. Built standalone (not as a meson
//! subproject) so its `.pc`/header land in the shared sysroot instead of
//! requiring network access to fetch a wrap subproject at fcft/foot's
//! own build time.

use anyhow::Context;
use buildpack_core::build::meson_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TllistConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Tllist {
    cfg: TllistConfig,
}

impl Tllist {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join("tllist")
    }
}

impl Buildpack for Tllist {
    fn id(&self) -> &'static str {
        "tllist"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [tllist] config")?;
        Ok(())
    }

    fn to_toml(&self) -> anyhow::Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [tllist] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    fn describe(&self) -> Description {
        Description {
            id: "tllist",
            name: "tllist",
            summary: "Header-only typed linked list — fcft/foot's dependency",
            long_description: "Meson build, header + pkgconfig install only, no compiled library.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("tllist-{}.tar.gz", self.cfg.version),
            extracted_dir_name: "tllist".to_string(),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let marker = ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/tllist.pc");
        if already_built(&marker, force) {
            println!("skip build-tllist: {} already exists", marker.display());
            return Ok(());
        }

        let dir = self.build_dir(ctx);
        println!("configuring/building/installing tllist in {}", dir.display());
        meson_build_and_install(ctx, &dir, &[])
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "tllist.pc (sysroot marker)".to_string(),
            path: ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/tllist.pc"),
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
