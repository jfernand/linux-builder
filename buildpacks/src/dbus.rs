//! dbus — the first genuinely dynamically-linked dependency in the
//! distro: it needs libexpat for XML parsing, which isn't practical to
//! statically link.

use anyhow::Context;
use buildpack_core::build::meson_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DbusConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Dbus {
    cfg: DbusConfig,
}

impl Dbus {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("dbus-{}", self.cfg.version))
    }
}

impl Buildpack for Dbus {
    fn id(&self) -> &'static str {
        "dbus"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [dbus] config")?;
        Ok(())
    }

    fn to_toml(&self) -> anyhow::Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [dbus] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    fn required(&self) -> bool {
        true // distro-init execs dbus-daemon unconditionally, no existence check
    }

    fn describe(&self) -> Description {
        Description {
            id: "dbus",
            name: "dbus",
            summary: "System message bus",
            long_description: "Meson build. No non-root user exists yet to drop privileges to, \
                so the daemon runs and stays as root; /run over the default /var/local/run so \
                the socket ends up where a standard system bus expects it.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("dbus-{}.tar.xz", self.cfg.version),
            extracted_dir_name: format!("dbus-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let marker = ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/dbus-1.pc");
        if already_built(&marker, force) {
            println!("skip build-dbus: {} already exists", marker.display());
            return Ok(());
        }

        let dir = self.build_dir(ctx);
        println!("configuring/building/installing dbus in {}", dir.display());
        meson_build_and_install(
            ctx,
            &dir,
            &[
                "-Druntime_dir=/run",
                "-Dsystem_socket=/run/dbus/system_bus_socket",
                "-Ddbus_user=root",
                "-Dsession_socket_dir=/tmp",
                "-Dsystemd=disabled",
                "-Dselinux=disabled",
                "-Dapparmor=disabled",
                "-Dlaunchd=disabled",
                "-Dx11_autolaunch=disabled",
                "-Ddoxygen_docs=disabled",
                "-Dxml_docs=disabled",
                "-Dqt_help=disabled",
                "-Dmodular_tests=disabled",
                "-Dasserts=false",
            ],
        )
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "dbus-1.pc (sysroot marker)".to_string(),
            path: ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/dbus-1.pc"),
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
