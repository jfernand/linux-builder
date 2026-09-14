//! Base Wayland: wire protocol libraries (client/server/cursor/egl) and
//! `wayland-scanner`, the code generator every later Wayland-protocol
//! package (wayland-protocols, Mesa, weston) invokes at its own build
//! time. No special-casing needed despite `wayland-scanner`'s path being
//! baked into wayland's `.pc` file as a custom variable
//! (`wayland_scanner=${bindir}/wayland-scanner`) — meson's own
//! `PkgConfigDependency` rewrites custom variables for
//! `PKG_CONFIG_SYSROOT_DIR` the same as ordinary `Cflags`/`Libs`.

use anyhow::Context;
use buildpack_core::build::meson_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct WaylandConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Wayland {
    cfg: WaylandConfig,
}

impl Wayland {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("wayland-{}", self.cfg.version))
    }
}

impl Buildpack for Wayland {
    fn id(&self) -> &'static str {
        "wayland"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [wayland] config")?;
        Ok(())
    }

    fn to_toml(&self) -> anyhow::Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [wayland] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    fn describe(&self) -> Description {
        Description {
            id: "wayland",
            name: "wayland",
            summary: "Core Wayland protocol libraries and wayland-scanner",
            long_description: "Meson build, docs/tests/dtd-validation disabled.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("wayland-{}.tar.xz", self.cfg.version),
            extracted_dir_name: format!("wayland-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let marker = ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/wayland-client.pc");
        if already_built(&marker, force) {
            println!("skip build-wayland: {} already exists", marker.display());
            return Ok(());
        }

        let dir = self.build_dir(ctx);
        println!("configuring/building/installing wayland in {}", dir.display());
        meson_build_and_install(
            ctx,
            &dir,
            &["-Ddocumentation=false", "-Dtests=false", "-Ddtd_validation=false"],
        )
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "wayland-client.pc (sysroot marker)".to_string(),
            path: ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/wayland-client.pc"),
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
