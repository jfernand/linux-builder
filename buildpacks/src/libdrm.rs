//! The kernel-userspace ioctl wrapper library Mesa (and every other
//! GPU-facing library) builds on. Every vendor-specific KMS API is
//! disabled — virtio-gpu needs only libdrm's generic core.

use anyhow::Context;
use buildpack_core::build::meson_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::Deserialize;
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct LibdrmConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Libdrm {
    cfg: LibdrmConfig,
}

impl Libdrm {
    pub fn new() -> Self {
        Self::default()
    }

    /// GitLab appends the tag's target commit SHA to libdrm's archive
    /// directory name specifically (unlike pixman's/libevdev's own clean
    /// `<name>-<name>-<version>/` GitLab-archive naming) — a per-project,
    /// per-tag quirk, not derivable from the version alone. Literal, not
    /// templated: bumping the version means updating this whole string,
    /// since the hash is only valid for the exact tag it names.
    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join("libdrm-libdrm-2.4.134-e984d448b8b17aab853369e6c203e53719f46de1")
    }
}

impl Buildpack for Libdrm {
    fn id(&self) -> &'static str {
        "libdrm"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [libdrm] config")?;
        Ok(())
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    fn describe(&self) -> Description {
        Description {
            id: "libdrm",
            name: "libdrm",
            summary: "Kernel-userspace DRM ioctl wrapper",
            long_description: "Meson build. Every vendor-specific sub-library \
                (Intel/AMD/nouveau/...) disabled — virtio-gpu needs only libdrm's generic core.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("libdrm-{}.tar.gz", self.cfg.version),
            extracted_dir_name: "libdrm-libdrm-2.4.134-e984d448b8b17aab853369e6c203e53719f46de1"
                .to_string(),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let marker = ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/libdrm.pc");
        if already_built(&marker, force) {
            println!("skip build-libdrm: {} already exists", marker.display());
            return Ok(());
        }

        let dir = self.build_dir(ctx);
        println!("configuring/building/installing libdrm in {}", dir.display());
        meson_build_and_install(
            ctx,
            &dir,
            &[
                "-Dintel=disabled",
                "-Dradeon=disabled",
                "-Damdgpu=disabled",
                "-Dnouveau=disabled",
                "-Dvmwgfx=disabled",
                "-Domap=disabled",
                "-Dexynos=disabled",
                "-Dfreedreno=disabled",
                "-Dtegra=disabled",
                "-Dvc4=disabled",
                "-Detnaviv=disabled",
                "-Dcairo-tests=disabled",
                "-Dman-pages=disabled",
                "-Dvalgrind=disabled",
                "-Dtests=false",
                "-Dudev=true",
            ],
        )
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "libdrm.pc (sysroot marker)",
            path: ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/libdrm.pc"),
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
