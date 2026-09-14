//! weston — Phase 3's Wayland compositor. Args copied verbatim from the
//! real, successfully-run manual scratch build (`meson-logs/meson-log.txt`'s
//! recorded "Build Options" line): DRM backend, GL renderer, kiosk shell,
//! `weston-simple-egl` only, no Vulkan/X11/Xwayland/systemd/jpeg/webp/
//! lcms2/docs/tests.

use anyhow::{bail, Context, Result};
use buildpack_core::build::meson_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source, SourcePatch};
use serde::Deserialize;
use std::any::Any;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WestonConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Weston {
    cfg: WestonConfig,
}

impl Weston {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("weston-{}", self.cfg.version))
    }
}

impl Buildpack for Weston {
    fn id(&self) -> &'static str {
        "weston"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [weston] config")?;
        Ok(())
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[
            "cairo",
            "libpng",
            "wayland",
            "wayland_protocols",
            "libxkbcommon",
            "pixman",
            "libdisplay_info",
            "libinput",
            "libdrm",
            "mesa",
            "seatd",
            "dbus",
            "eudev",
        ]
    }

    fn describe(&self) -> Description {
        Description {
            id: "weston",
            name: "Weston",
            summary: "Wayland compositor — DRM backend, GL renderer, kiosk shell, weston-simple-egl",
            long_description: "Phase 3's actual milestone: a minimal Wayland client \
                (weston-simple-egl) rendering via QEMU's virtio-gpu. Vulkan/X11/Xwayland/\
                systemd/jpeg/webp/lcms2/docs/tests all disabled.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("weston-{}.tar.gz", self.cfg.version),
            extracted_dir_name: format!("weston-{}", self.cfg.version),
        }]
    }

    fn patches(&self, _ctx: &BuildCtx) -> Vec<SourcePatch> {
        vec![SourcePatch {
            description: "disable weston's unconditional pango/glib probe in shared/meson.build",
            apply: patch_disable_pango,
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let dir = self.build_dir(ctx);
        // The sysroot-installed .pc, not the build-dir-local meson-private
        // copy: meson's pkgconfig-generation step can complete (and write
        // that file) independently of, and before, other targets in the
        // same ninja run — so on a partially-failed build it's a false
        // "already built" signal. Only `ninja install` (which never runs
        // if any earlier step failed) writes the sysroot copy, making it
        // the reliable marker.
        let marker = ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/libweston-16.pc");

        if already_built(&marker, force) {
            println!("skip build-weston: {} already exists", marker.display());
            return Ok(());
        }

        println!("configuring/building/installing weston in {}", dir.display());
        meson_build_and_install(
            ctx,
            &dir,
            &[
                "-Dbackend-drm=true",
                "-Dbackend-headless=false",
                "-Dbackend-pipewire=false",
                "-Dbackend-rdp=false",
                "-Dbackend-vnc=false",
                "-Dbackend-wayland=false",
                "-Dbackend-x11=false",
                "-Drenderer-gl=true",
                "-Drenderer-vulkan=false",
                "-Dshell-desktop=false",
                "-Dshell-ivi=false",
                "-Dshell-kiosk=true",
                "-Dshell-lua=false",
                "-Dsimple-clients=egl",
                "-Ddemo-clients=false",
                "-Dtools=[]",
                "-Dxwayland=false",
                "-Dsystemd=false",
                "-Dcolor-management-lcms=false",
                "-Dimage-jpeg=false",
                "-Dimage-webp=false",
                "-Ddoc=false",
                "-Dtests=false",
            ],
        )
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        // Confirmed by an actual successful build (both land directly in
        // usr/bin, not libexecdir).
        vec![
            BuildOutput {
                description: "libweston-16.pc (sysroot marker)".to_string(),
                path: ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/libweston-16.pc"),
                rootfs_install: None,
            },
            BuildOutput {
                description: "weston compositor binary".to_string(),
                path: ctx.sysroot_dir.join("usr/bin/weston"),
                rootfs_install: None,
            },
            BuildOutput {
                description: "weston-simple-egl demo client".to_string(),
                path: ctx.sysroot_dir.join("usr/bin/weston-simple-egl"),
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

/// `shared/meson.build` unconditionally probes for pango/pangocairo/
/// fontconfig/glib-2.0 (all `required: false`, no meson option to skip
/// the probe) and, if ALL FOUR are found, adds them to `cairo-util.c`'s
/// dependencies and sets `HAVE_PANGO`. This host has apt-installed pango/
/// glib dev packages at genuine standard locations — found via
/// pkg-config's normal host-default search (not the sysroot) — so the
/// probe succeeds, `cairo-util.h` ends up `#include <pango/pangocairo.h>`,
/// and the build fails with a missing header (none of pango/glib/
/// harfbuzz/fribidi are buildpacks; out of scope, and genuinely
/// unneeded — weston's kiosk-shell frame decoration works fine with
/// cairo's plain toy-font API). Same bug class, same fix pattern as
/// `patch_libdisplay_info_hwdata`: force the `if` to never fire.
fn patch_disable_pango(dir: &Path) -> Result<()> {
    let path = dir.join("shared").join("meson.build");
    let mut text = std::fs::read_to_string(&path)
        .with_context(|| format!("reading {}", path.display()))?;

    let already_patched = "if false # pango/glib disabled by distro's weston buildpack";
    if text.contains(already_patched) {
        return Ok(());
    }

    let original = "if dep_pango.found() and dep_pangocairo.found() and dep_fontconfig.found() and dep_glib.found()";
    if !text.contains(original) {
        bail!(
            "expected pango-probe text not found in {} — weston's shared/meson.build may have changed upstream",
            path.display()
        );
    }
    text = text.replace(original, already_patched);

    std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    println!("patched {} to disable the pango/glib probe", path.display());
    Ok(())
}
