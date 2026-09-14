//! xkeyboard-config — pure data (keyboard layout/rules XML+lua, no C
//! code, no shared library). Not a build-time dependency of anything;
//! it's what libxkbcommon looks for *at runtime* under
//! `/usr/share/X11/xkb` — its absence is exactly what made weston fail
//! immediately with `xkbcommon: ERROR: failed to add default include
//! path /usr/share/X11/xkb` / `failed to create XKB context` in a real
//! QEMU boot test, the actual last blocker for Phase 3's milestone.

use anyhow::Context;
use buildpack_core::build::meson_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct XkeyboardConfigConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct XkeyboardConfig {
    cfg: XkeyboardConfigConfig,
}

impl XkeyboardConfig {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("xkeyboard-config-{}", self.cfg.version))
    }
}

impl Buildpack for XkeyboardConfig {
    fn id(&self) -> &'static str {
        "xkeyboard_config"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [xkeyboard_config] config")?;
        Ok(())
    }

    fn to_toml(&self) -> anyhow::Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [xkeyboard_config] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    fn describe(&self) -> Description {
        Description {
            id: "xkeyboard_config",
            name: "xkeyboard-config",
            summary: "XKB keyboard layout/rules data — libxkbcommon's runtime dependency",
            long_description: "Pure data package, no C code. Installs to \
                /usr/share/X11/xkb, the hardcoded default path libxkbcommon looks for \
                at runtime (not build time) to actually compile a keymap.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("xkeyboard-config-{}.tar.xz", self.cfg.version),
            extracted_dir_name: format!("xkeyboard-config-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let dir = self.build_dir(ctx);
        let marker = ctx.sysroot_dir.join("usr/share/X11/xkb/rules/base.lst");

        if already_built(&marker, force) {
            println!("skip build-xkeyboard-config: {} already exists", marker.display());
            return Ok(());
        }

        println!("configuring/building/installing xkeyboard-config in {}", dir.display());
        meson_build_and_install(ctx, &dir, &["-Dnls=false"])
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "base.lst (sysroot marker)".to_string(),
            path: ctx.sysroot_dir.join("usr/share/X11/xkb/rules/base.lst"),
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
