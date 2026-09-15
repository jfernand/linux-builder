//! The Mesa buildpack — meson, `InstallMode::Sysroot`, and the deepest
//! `dependencies()` list of the graphics stack. Ported from
//! `distro/src/stages/userland.rs`'s `build_mesa`. The
//! `PKG_CONFIG=/usr/bin/pkg-config` env override that dodges a
//! Homebrew-pkgconfig-leak bug class is applied uniformly by
//! `buildpack_core::build::sysroot_env`, not per-package here — see that
//! function's doc comment.

use anyhow::Context;
use buildpack_core::build::meson_build_and_install_env;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MesaConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Mesa {
    cfg: MesaConfig,
}

impl Mesa {
    pub fn new() -> Self {
        Self::default()
    }

    /// Matches `default_fetch`'s extraction target — see `Kernel::build_dir`'s
    /// doc comment for why these must agree.
    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("mesa-mesa-{}", self.cfg.version))
    }
}

impl Buildpack for Mesa {
    fn id(&self) -> &'static str {
        "mesa"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [mesa] config")?;
        Ok(())
    }

    fn to_toml(&self) -> anyhow::Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [mesa] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &["libdrm", "wayland", "libxkbcommon", "pixman", "libdisplay_info", "libinput"]
    }

    fn describe(&self) -> Description {
        Description {
            id: "mesa",
            name: "Mesa",
            summary: "OpenGL/EGL/GBM drivers (virgl/softpipe) plus lavapipe, Mesa's software Vulkan driver",
            long_description: "Built with meson, platforms=wayland, gallium-drivers=virgl,\
                softpipe, vulkan-drivers=swrast (lavapipe — no host GPU/Vulkan ICD dependency,\
                same portability reasoning as softpipe). LLVM statically linked in\
                (shared-llvm=disabled), so the target rootfs carries no separate libLLVM.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("mesa-{}.tar.xz", self.cfg.version),
            // GitLab double-naming: mesa-mesa-<version>/, same quirk as
            // pixman/libevdev's archives.
            extracted_dir_name: format!("mesa-mesa-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let dir = self.build_dir(ctx);
        // Sysroot-installed .pc, not the build-dir-local meson-private
        // copy — see buildpacks::weston::Weston::build's doc comment for
        // why (a false "already built" signal on a partial failed build).
        let marker = ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/gbm.pc");

        if already_built(&marker, force) {
            println!("skip build-mesa: {} already exists", marker.display());
            return Ok(());
        }

        println!("configuring/building/installing mesa in {}", dir.display());
        // Mesa's LLVM detection walks PATH for `llvm-config` — this host's
        // Homebrew install shadows the correct /usr/bin one (18.1.3) with
        // an incompatible major version (22.1.8) ahead of it in PATH, the
        // same leak class sysroot_env's own PKG_CONFIG override already
        // guards against for pkg-config. Prepending /usr/bin here (ahead
        // of the rest of the host PATH, but after ~/.local/bin, which
        // still needs to win for meson itself — see the toolchain note
        // above) forces the right one without disturbing anything else.
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let local_bin = home
            .as_ref()
            .map(|h| h.join(".local/bin"))
            .filter(|p| p.exists())
            .map(|p| format!("{}:", p.display()))
            .unwrap_or_default();
        let path = format!("{local_bin}/usr/bin:/usr/sbin:{}", std::env::var("PATH").unwrap_or_default());

        meson_build_and_install_env(
            ctx,
            &dir,
            &[
                "-Dplatforms=wayland",
                "-Dgallium-drivers=virgl,softpipe",
                "-Dvulkan-drivers=swrast",
                "-Dllvm=enabled",
                "-Dshared-llvm=disabled",
                "-Dglx=disabled",
                "-Dgbm=enabled",
                "-Degl=enabled",
                "-Dopengl=true",
                "-Dgles1=disabled",
                "-Dgles2=enabled",
                "-Dspirv-tools=disabled",
                "-Dlmsensors=disabled",
            ],
            &[("PATH", &path)],
        )
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "libEGL_mesa.so (sysroot marker)".to_string(),
            path: ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/libEGL_mesa.so"),
            rootfs_install: None, // Sysroot mode: bulk-copied, not installed individually
        }]
    }

    fn install_mode(&self) -> InstallMode {
        InstallMode::Sysroot
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
