//! wlroots — sway's compositor library. Needs zero new C libraries: every
//! dependency it probes for (EGL/GBM/GLESv2 from Mesa, libseat from
//! `seatd`, libdisplay-info, libudev, libdrm, xkbcommon, pixman,
//! wayland-server/wayland-protocols) is already in this sysroot. `hwdata`
//! (DRM connector vendor-name lookup) is a `native: true` build-time-only
//! probe wlroots itself declares — resolved from the host's own apt
//! package, same exemption as every other host build tool here.

use anyhow::Context;
use buildpack_core::build::meson_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct WlrootsConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Wlroots {
    cfg: WlrootsConfig,
}

impl Wlroots {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("wlroots-{}", self.cfg.version))
    }
}

impl Buildpack for Wlroots {
    fn id(&self) -> &'static str {
        "wlroots"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [wlroots] config")?;
        Ok(())
    }

    fn to_toml(&self) -> anyhow::Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [wlroots] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[
            "wayland",
            "wayland_protocols",
            "libxkbcommon",
            "pixman",
            "libdrm",
            "mesa",
            "libinput",
            "libdisplay_info",
            "seatd",
            "eudev",
        ]
    }

    fn describe(&self) -> Description {
        Description {
            id: "wlroots",
            name: "wlroots",
            summary: "Wayland compositor library — sway is built on top of this",
            long_description: "Meson build. DRM+libinput backends, gles2 renderer, gbm \
                allocator, session support all enabled; X11 backend/Xwayland/vulkan-renderer/\
                color-management/libliftoff/examples all disabled.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("wlroots-{}.tar.gz", self.cfg.version),
            extracted_dir_name: format!("wlroots-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let marker = ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/wlroots-0.20.pc");
        if already_built(&marker, force) {
            println!("skip build-wlroots: {} already exists", marker.display());
            return Ok(());
        }

        // wlroots' DRM backend reads hwdata's pnp.ids (vendor-name table)
        // at build time via a `native: true` dependency — but this
        // workspace's PKG_CONFIG_SYSROOT_DIR (needed so every OTHER
        // pkg-config lookup here finds this shared sysroot, not the
        // host) still mangles hwdata's own pkgdatadir variable with that
        // same sysroot prefix, since meson's "native" and "host" pkg-config
        // are the same single invocation in a non-cross build. Same bug
        // class as weston's pango probe/Mesa's llvm-config (see those
        // buildpacks' own comments) — staging a copy at the path the
        // mangled lookup actually resolves to, rather than fighting
        // sysroot_env's global PKG_CONFIG_SYSROOT_DIR for one package.
        let hwdata_dst = ctx.sysroot_dir.join("usr/share/hwdata/pnp.ids");
        if !hwdata_dst.exists() {
            let hwdata_src = PathBuf::from("/usr/share/hwdata/pnp.ids");
            std::fs::create_dir_all(hwdata_dst.parent().unwrap())
                .context("creating sysroot's usr/share/hwdata dir")?;
            std::fs::copy(&hwdata_src, &hwdata_dst).with_context(|| {
                format!("copying {} to {}", hwdata_src.display(), hwdata_dst.display())
            })?;
        }

        let dir = self.build_dir(ctx);
        println!("configuring/building/installing wlroots in {}", dir.display());
        meson_build_and_install(
            ctx,
            &dir,
            &[
                "-Dbackends=drm,libinput",
                "-Dxwayland=disabled",
                "-Dxcb-errors=disabled",
                "-Drenderers=gles2",
                "-Dallocators=gbm",
                "-Dsession=enabled",
                "-Dcolor-management=disabled",
                "-Dlibliftoff=disabled",
                "-Dexamples=false",
            ],
        )
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "wlroots-0.20.pc (sysroot marker)".to_string(),
            path: ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/wlroots-0.20.pc"),
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
