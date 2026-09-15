//! Vulkan-Headers — the Vulkan API definition headers (`vulkan/vulkan.h`
//! and friends) plus a `vulkan.pc`. Header-only: no library to link, no
//! binary produced, but genuinely this distro's own copy of the API
//! surface its own Vulkan loader (§10.3.6's `cosmic-comp` needs it at
//! runtime) and Mesa's Vulkan driver both build against — built from
//! source the same as every other buildpack here, not reached for via
//! `apt install vulkan-headers` (which doesn't exist as a package on
//! this host's Ubuntu release anyway).

use anyhow::Context;
use buildpack_core::build::cmake_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct VulkanHeadersConfig {
    pub git_url: String,
    pub git_rev: String,
}

#[derive(Default)]
pub struct VulkanHeaders {
    cfg: VulkanHeadersConfig,
}

impl VulkanHeaders {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join("Vulkan-Headers")
    }
}

impl Buildpack for VulkanHeaders {
    fn id(&self) -> &'static str {
        "vulkan_headers"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [vulkan_headers] config")?;
        Ok(())
    }

    fn to_toml(&self) -> anyhow::Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [vulkan_headers] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    fn describe(&self) -> Description {
        Description {
            id: "vulkan_headers",
            name: "Vulkan-Headers",
            summary: "The Vulkan API headers — build-time only, no library",
            long_description: "CMake build, header-only install. What Vulkan-Loader and Mesa's \
                Vulkan driver both build against.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Git {
            url: self.cfg.git_url.clone(),
            rev: self.cfg.git_rev.clone(),
            checkout_dir_name: "Vulkan-Headers".to_string(),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let marker = ctx.sysroot_dir.join("usr/share/pkgconfig/vulkan.pc");
        if already_built(&marker, force) {
            println!("skip build-vulkan-headers: {} already exists", marker.display());
            return Ok(());
        }

        let dir = self.build_dir(ctx);
        println!("configuring/building/installing vulkan-headers in {}", dir.display());
        cmake_build_and_install(ctx, &dir, &[])
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "vulkan.pc (sysroot marker)".to_string(),
            path: ctx.sysroot_dir.join("usr/share/pkgconfig/vulkan.pc"),
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
