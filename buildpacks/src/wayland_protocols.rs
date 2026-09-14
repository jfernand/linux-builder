//! The Wayland protocol XML definitions (xdg-shell and friends) — no
//! library, just data plus a pkg-config file weston's build reads to
//! find them. Needs `wayland-scanner` (from the `wayland` buildpack) at
//! its own build time to validate/process a couple of them.

use anyhow::Context;
use buildpack_core::build::meson_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct WaylandProtocolsConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct WaylandProtocols {
    cfg: WaylandProtocolsConfig,
}

impl WaylandProtocols {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("wayland-protocols-{}", self.cfg.version))
    }
}

impl Buildpack for WaylandProtocols {
    fn id(&self) -> &'static str {
        "wayland_protocols"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [wayland_protocols] config")?;
        Ok(())
    }

    fn to_toml(&self) -> anyhow::Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [wayland_protocols] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &["wayland"]
    }

    fn describe(&self) -> Description {
        Description {
            id: "wayland_protocols",
            name: "wayland-protocols",
            summary: "Wayland protocol XML definitions (xdg-shell and friends)",
            long_description: "Meson build, tests disabled. Data + a pkg-config file only.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("wayland-protocols-{}.tar.xz", self.cfg.version),
            extracted_dir_name: format!("wayland-protocols-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let marker = ctx.sysroot_dir.join("usr/share/pkgconfig/wayland-protocols.pc");
        if already_built(&marker, force) {
            println!("skip build-wayland-protocols: {} already exists", marker.display());
            return Ok(());
        }

        let dir = self.build_dir(ctx);
        println!("configuring/building/installing wayland-protocols in {}", dir.display());
        meson_build_and_install(ctx, &dir, &["-Dtests=false"])
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "wayland-protocols.pc (sysroot marker)".to_string(),
            path: ctx.sysroot_dir.join("usr/share/pkgconfig/wayland-protocols.pc"),
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
