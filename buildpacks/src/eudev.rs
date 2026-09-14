//! eudev — a systemd-independent fork of udev, needed for libudev, which
//! libinput hard-depends on. Unlike seatd/dbus it's autotools, not meson.

use anyhow::Context;
use buildpack_core::build::autotools_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::Deserialize;
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct EudevConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Eudev {
    cfg: EudevConfig,
}

impl Eudev {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("eudev-{}", self.cfg.version))
    }
}

impl Buildpack for Eudev {
    fn id(&self) -> &'static str {
        "eudev"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [eudev] config")?;
        Ok(())
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    fn describe(&self) -> Description {
        Description {
            id: "eudev",
            name: "eudev",
            summary: "systemd-independent udev fork — libinput's hard dependency for libudev",
            long_description: "Autotools build. blkid/SELinux/kmod support all disabled. \
                --with-rootlibexecdir=/usr/lib/udev is explicit, not auto-detected: this host's \
                genuinely-installed systemd-dev package's udev.pc otherwise gets found via \
                pkg-config and PKG_CONFIG_SYSROOT_DIR rewrites its rules-directory variable into \
                a literal build-machine absolute path baked into udevd, which silently works on \
                the build machine but finds zero rules once booted as an independent image.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("eudev-{}.tar.gz", self.cfg.version),
            extracted_dir_name: format!("eudev-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let marker = ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/libudev.pc");
        if already_built(&marker, force) {
            println!("skip build-eudev: {} already exists", marker.display());
            return Ok(());
        }

        let dir = self.build_dir(ctx);
        println!("configuring/building/installing eudev in {}", dir.display());
        autotools_build_and_install(
            ctx,
            &dir,
            &[
                "--sysconfdir=/etc",
                "--libdir=/usr/lib/x86_64-linux-gnu",
                "--with-rootlibexecdir=/usr/lib/udev",
                "--disable-blkid",
                "--disable-selinux",
                "--disable-kmod",
                "--disable-manpages",
            ],
        )
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "libudev.pc (sysroot marker)".to_string(),
            path: ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/libudev.pc"),
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
