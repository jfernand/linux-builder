//! seatd — the first thing in the pipeline built with meson/ninja instead
//! of autotools, and, per its own README, "Depends only on libc," so it
//! could in principle still be statically linked. Built dynamically
//! anyway for consistency with dbus and everything after it (Phase 2's
//! switch away from Phase 1's all-static approach).

use anyhow::Context;
use buildpack_core::build::meson_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::Deserialize;
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SeatdConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Seatd {
    cfg: SeatdConfig,
}

impl Seatd {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("seatd-{}", self.cfg.version))
    }
}

impl Buildpack for Seatd {
    fn id(&self) -> &'static str {
        "seatd"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [seatd] config")?;
        Ok(())
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    fn describe(&self) -> Description {
        Description {
            id: "seatd",
            name: "seatd",
            summary: "Seat management daemon — grants exclusive device access to one session at a time",
            long_description: "Meson build. libseat-logind disabled (no systemd), \
                libseat-seatd enabled.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("seatd-{}.tar.gz", self.cfg.version),
            extracted_dir_name: format!("seatd-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let marker = ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/libseat.pc");
        if already_built(&marker, force) {
            println!("skip build-seatd: {} already exists", marker.display());
            return Ok(());
        }

        let dir = self.build_dir(ctx);
        println!("configuring/building/installing seatd in {}", dir.display());
        meson_build_and_install(
            ctx,
            &dir,
            &[
                "-Dlibseat-logind=disabled",
                "-Dlibseat-seatd=enabled",
                "-Dserver=enabled",
                "-Dman-pages=disabled",
                "-Dexamples=disabled",
            ],
        )
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "libseat.pc (sysroot marker)".to_string(),
            path: ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/libseat.pc"),
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
