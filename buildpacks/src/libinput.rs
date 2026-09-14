//! Turns raw evdev events into the pointer/keyboard/touch/gesture events
//! a compositor actually wants. libwacom (tablets) and mtdev (legacy
//! multitouch) both deliberately skipped: niche hardware support not
//! worth two more from-source packages. Lua plugin support is off too
//! (no lua on the target).

use anyhow::Context;
use buildpack_core::build::meson_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct LibinputConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Libinput {
    cfg: LibinputConfig,
}

impl Libinput {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("libinput-{}", self.cfg.version))
    }
}

impl Buildpack for Libinput {
    fn id(&self) -> &'static str {
        "libinput"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [libinput] config")?;
        Ok(())
    }

    fn to_toml(&self) -> anyhow::Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [libinput] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &["eudev", "libevdev"]
    }

    fn describe(&self) -> Description {
        Description {
            id: "libinput",
            name: "libinput",
            summary: "Pointer/keyboard/touch/gesture events from raw evdev",
            long_description: "Meson build. libwacom/mtdev/lua-plugins all disabled — niche \
                hardware support not worth extra packages for now.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("libinput-{}.tar.gz", self.cfg.version),
            extracted_dir_name: format!("libinput-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let marker = ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/libinput.pc");
        if already_built(&marker, force) {
            println!("skip build-libinput: {} already exists", marker.display());
            return Ok(());
        }

        let dir = self.build_dir(ctx);
        println!("configuring/building/installing libinput in {}", dir.display());
        meson_build_and_install(
            ctx,
            &dir,
            &[
                "-Dlibwacom=false",
                "-Dmtdev=false",
                "-Ddebug-gui=false",
                "-Dtests=false",
                "-Ddocumentation=false",
                "-Dlua-plugins=disabled",
            ],
        )
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "libinput.pc (sysroot marker)".to_string(),
            path: ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/libinput.pc"),
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
