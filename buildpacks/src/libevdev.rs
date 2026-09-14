//! libinput's mandatory dependency for reading/writing raw evdev input
//! devices.

use anyhow::Context;
use buildpack_core::build::meson_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct LibevdevConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Libevdev {
    cfg: LibevdevConfig,
}

impl Libevdev {
    pub fn new() -> Self {
        Self::default()
    }

    /// Same GitLab-archive double-naming: `libevdev-libevdev-<version>/`.
    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("libevdev-libevdev-{}", self.cfg.version))
    }
}

impl Buildpack for Libevdev {
    fn id(&self) -> &'static str {
        "libevdev"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [libevdev] config")?;
        Ok(())
    }

    fn to_toml(&self) -> anyhow::Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [libevdev] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    fn describe(&self) -> Description {
        Description {
            id: "libevdev",
            name: "libevdev",
            summary: "Raw evdev input device wrapper — libinput's mandatory dependency",
            long_description: "Meson build, tests/documentation disabled.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("libevdev-{}.tar.gz", self.cfg.version),
            extracted_dir_name: format!("libevdev-libevdev-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let marker = ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/libevdev.pc");
        if already_built(&marker, force) {
            println!("skip build-libevdev: {} already exists", marker.display());
            return Ok(());
        }

        let dir = self.build_dir(ctx);
        println!("configuring/building/installing libevdev in {}", dir.display());
        meson_build_and_install(ctx, &dir, &["-Dtests=disabled", "-Ddocumentation=disabled"])
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "libevdev.pc (sysroot marker)".to_string(),
            path: ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/libevdev.pc"),
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
