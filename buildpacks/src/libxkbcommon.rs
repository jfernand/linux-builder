//! Keymap compilation (turns e.g. "us, evdev, pc105" into the lookup
//! tables a compositor hands to clients). X11 support disabled (no
//! libxcb); `xkb-config-root` pinned to the FHS-standard
//! `/usr/share/X11/xkb` (the `xkeyboard-config` buildpack's own install
//! location) rather than whatever meson's default host-pkg-config probe
//! would bake in.

use anyhow::Context;
use buildpack_core::build::meson_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct LibxkbcommonConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Libxkbcommon {
    cfg: LibxkbcommonConfig,
}

impl Libxkbcommon {
    pub fn new() -> Self {
        Self::default()
    }

    /// GitHub's tag archive nests an extra `libxkbcommon-` prefix onto the
    /// tag name (`libxkbcommon-xkbcommon-1.12.4/`), unlike most other
    /// dependencies' tarballs.
    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("libxkbcommon-xkbcommon-{}", self.cfg.version))
    }
}

impl Buildpack for Libxkbcommon {
    fn id(&self) -> &'static str {
        "libxkbcommon"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [libxkbcommon] config")?;
        Ok(())
    }

    fn to_toml(&self) -> anyhow::Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [libxkbcommon] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    fn describe(&self) -> Description {
        Description {
            id: "libxkbcommon",
            name: "libxkbcommon",
            summary: "Keymap compilation library",
            long_description: "Meson build. X11 and xkbregistry (needs libxml2, not built) \
                both disabled; xkb-config-root pinned to /usr/share/X11/xkb.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("libxkbcommon-{}.tar.gz", self.cfg.version),
            extracted_dir_name: format!("libxkbcommon-xkbcommon-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let marker = ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/xkbcommon.pc");
        if already_built(&marker, force) {
            println!("skip build-libxkbcommon: {} already exists", marker.display());
            return Ok(());
        }

        let dir = self.build_dir(ctx);
        println!("configuring/building/installing libxkbcommon in {}", dir.display());
        meson_build_and_install(
            ctx,
            &dir,
            &[
                "-Denable-x11=false",
                "-Denable-docs=false",
                "-Dxkb-config-root=/usr/share/X11/xkb",
                "-Denable-xkbregistry=false",
            ],
        )
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "xkbcommon.pc (sysroot marker)".to_string(),
            path: ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/xkbcommon.pc"),
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
