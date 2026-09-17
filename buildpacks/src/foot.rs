//! foot — a wlroots-native Wayland terminal emulator, tried here as an
//! alternative to Alacritty (§10.3.6.5's still-open winit/Wayland bug)
//! and cosmic-term (§10.3.6.6's iced/winit panic): much lighter than
//! either, no pango/glib, just its own `fcft` font-rasterization library.

use anyhow::Context;
use buildpack_core::build::meson_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct FootConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Foot {
    cfg: FootConfig,
}

impl Foot {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join("foot")
    }
}

impl Buildpack for Foot {
    fn id(&self) -> &'static str {
        "foot"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [foot] config")?;
        Ok(())
    }

    fn to_toml(&self) -> anyhow::Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [foot] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &["wayland", "wayland_protocols", "libxkbcommon", "pixman", "fontconfig", "fcft", "tllist"]
    }

    fn functional_dependencies(&self) -> &'static [&'static str] {
        // Inert without a compositor to connect to, same as every other
        // Wayland client here.
        &["cosmic_comp", "sway", "dejavu_fonts"]
    }

    fn describe(&self) -> Description {
        Description {
            id: "foot",
            name: "foot",
            summary: "Lightweight Wayland-native terminal emulator",
            long_description: "Meson build. grapheme-clustering (needs libutf8proc, not built), \
                docs (needs scdoc), and tests all disabled; utmp logging backend set to none \
                (no libutempter). Reuses fcft for font rendering instead of pango — no glib \
                dependency at all, unlike sway itself.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("foot-{}.tar.gz", self.cfg.version),
            extracted_dir_name: "foot".to_string(),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let marker = ctx.sysroot_dir.join("usr/bin/foot");
        if already_built(&marker, force) {
            println!("skip build-foot: {} already exists", marker.display());
            return Ok(());
        }

        let dir = self.build_dir(ctx);
        println!("configuring/building/installing foot in {}", dir.display());
        meson_build_and_install(
            ctx,
            &dir,
            &[
                "-Dgrapheme-clustering=disabled",
                "-Ddocs=disabled",
                "-Dtests=false",
                // foot's own meson.build only skips installing systemd user
                // units when BOTH this is empty AND `dependency('systemd')`
                // isn't found — but that probe leaks the host's own real
                // systemd.pc (this build host has one; the built image has
                // no systemd at all), and PKG_CONFIG_SYSROOT_DIR then
                // mangles its already-absolute completionsdir-style
                // variable, which ninja's DESTDIR install step prefixes
                // *again* on top of — installing to a doubled, garbage path
                // like sysroot/data/RustroverProjects/.../sysroot/usr/lib/…
                // instead of sysroot/usr/lib/…. Same bug class as weston's
                // pango probe/wlroots' hwdata/libxkbcommon's bash-completion
                // below. Overriding with an explicit, correct, DESTDIR-
                // relative path sidesteps the host probe entirely — these
                // unit files are unused in this dbus/seatd-only rootfs
                // either way, but at least land somewhere sane now.
                "-Dsystemd-units-dir=/usr/lib/systemd/user",
                "-Dutmp-backend=none",
            ],
        )
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "foot binary (sysroot marker)".to_string(),
            path: ctx.sysroot_dir.join("usr/bin/foot"),
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
