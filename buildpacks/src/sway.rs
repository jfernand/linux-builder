//! sway — the tiling Wayland compositor/WM chosen as an alternative to
//! COSMIC (§10.3.6.6's cosmic-term crash, Alacritty's still-open winit
//! bug): mature, wlroots-based, C throughout, no GTK/X11 wall like Ghostty.

use anyhow::Context;
use buildpack_core::build::meson_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SwayConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Sway {
    cfg: SwayConfig,
}

impl Sway {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("sway-{}", self.cfg.version))
    }
}

impl Buildpack for Sway {
    fn id(&self) -> &'static str {
        "sway"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [sway] config")?;
        Ok(())
    }

    fn to_toml(&self) -> anyhow::Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [sway] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[
            "wlroots",
            "json_c",
            "pcre2",
            "wayland",
            "wayland_protocols",
            "libxkbcommon",
            "cairo",
            "pango",
            "pixman",
            "libevdev",
            "libinput",
            "libdrm",
            "eudev",
        ]
    }

    fn describe(&self) -> Description {
        Description {
            id: "sway",
            name: "sway",
            summary: "Tiling Wayland compositor/WM, built on wlroots",
            long_description: "Meson build. gdk-pixbuf (swaybar tray image formats), tray/\
                sd-bus (systemd/elogind/basu), and man-pages (scdoc) all disabled — this is a \
                headless-console image with no D-Bus session tray or man command.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("sway-{}.tar.gz", self.cfg.version),
            extracted_dir_name: format!("sway-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let marker = ctx.sysroot_dir.join("usr/bin/sway");
        if already_built(&marker, force) {
            println!("skip build-sway: {} already exists", marker.display());
            return Ok(());
        }

        let dir = self.build_dir(ctx);
        println!("configuring/building/installing sway in {}", dir.display());
        meson_build_and_install(
            ctx,
            &dir,
            &[
                "-Dgdk-pixbuf=disabled",
                "-Dtray=disabled",
                "-Dman-pages=disabled",
                "-Ddefault-wallpaper=false",
                "-Dzsh-completions=false",
                "-Dbash-completions=false",
                "-Dfish-completions=false",
            ],
        )
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![
            BuildOutput {
                description: "sway binary (sysroot marker)".to_string(),
                path: ctx.sysroot_dir.join("usr/bin/sway"),
                rootfs_install: None,
            },
            BuildOutput {
                description: "swaybar binary".to_string(),
                path: ctx.sysroot_dir.join("usr/bin/swaybar"),
                rootfs_install: None,
            },
            BuildOutput {
                description: "swaynag binary".to_string(),
                path: ctx.sysroot_dir.join("usr/bin/swaynag"),
                rootfs_install: None,
            },
        ]
    }

    fn install_mode(&self) -> InstallMode {
        InstallMode::Sysroot
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
