//! json-c — sway's IPC message (de)serialization library.

use anyhow::Context;
use buildpack_core::build::cmake_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct JsonCConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct JsonC {
    cfg: JsonCConfig,
}

impl JsonC {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        // json-c's release tags are named "json-c-<version>", so GitHub's
        // archive tarball extracts to a doubled-up "json-c-json-c-<version>".
        ctx.sources_dir.join(format!("json-c-json-c-{}", self.cfg.version))
    }
}

impl Buildpack for JsonC {
    fn id(&self) -> &'static str {
        "json_c"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [json_c] config")?;
        Ok(())
    }

    fn to_toml(&self) -> anyhow::Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [json_c] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    fn describe(&self) -> Description {
        Description {
            id: "json_c",
            name: "json-c",
            summary: "JSON (de)serialization library — sway's IPC protocol dependency",
            long_description: "CMake build, static-lib-only tooling (tests/docs) disabled.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("json-c-{}.tar.gz", self.cfg.version),
            extracted_dir_name: format!("json-c-json-c-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let marker = ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/json-c.pc");
        if already_built(&marker, force) {
            println!("skip build-json-c: {} already exists", marker.display());
            return Ok(());
        }

        let dir = self.build_dir(ctx);
        println!("configuring/building/installing json-c in {}", dir.display());
        cmake_build_and_install(
            ctx,
            &dir,
            &[
                "-DBUILD_SHARED_LIBS=ON",
                "-DBUILD_STATIC_LIBS=OFF",
                "-DBUILD_TESTING=OFF",
                "-DDISABLE_EXTRA_LIBS=ON",
                "-DDISABLE_BSYMBOLIC=OFF",
            ],
        )
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "json-c.pc (sysroot marker)".to_string(),
            path: ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/json-c.pc"),
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
