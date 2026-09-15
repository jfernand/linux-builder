//! Vulkan-Loader — the actual `libvulkan.so.1` a Vulkan application
//! (e.g. `cosmic-comp`, via the `ash` crate, §10.3.6) `dlopen()`s at
//! runtime. Reads the ICD manifests under `/usr/share/vulkan/icd.d/*`
//! (Mesa's own build installs `lvp_icd.x86_64.json` there, §10.3.4) and
//! dispatches into whichever ICD a call actually targets — Mesa's
//! `libvulkan_lvp.so` is the driver; this is the dispatcher in front of
//! it. Without this, having a Vulkan ICD built is inert: nothing on the
//! system knows how to find or load it.

use anyhow::Context;
use buildpack_core::build::cmake_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct VulkanLoaderConfig {
    pub git_url: String,
    pub git_rev: String,
}

#[derive(Default)]
pub struct VulkanLoader {
    cfg: VulkanLoaderConfig,
}

impl VulkanLoader {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join("Vulkan-Loader")
    }
}

impl Buildpack for VulkanLoader {
    fn id(&self) -> &'static str {
        "vulkan_loader"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [vulkan_loader] config")?;
        Ok(())
    }

    fn to_toml(&self) -> anyhow::Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [vulkan_loader] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &["vulkan_headers"]
    }

    fn describe(&self) -> Description {
        Description {
            id: "vulkan_loader",
            name: "Vulkan-Loader",
            summary: "libvulkan.so.1 — dispatches Vulkan calls to whichever ICD a system has installed",
            long_description: "CMake build against Vulkan-Headers. The Khronos reference loader — \
                not a driver itself, the piece that finds and dlopen()s one via the ICD manifests \
                under /usr/share/vulkan/icd.d.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Git {
            url: self.cfg.git_url.clone(),
            rev: self.cfg.git_rev.clone(),
            checkout_dir_name: "Vulkan-Loader".to_string(),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let marker = ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/libvulkan.so.1");
        if already_built(&marker, force) {
            println!("skip build-vulkan-loader: {} already exists", marker.display());
            return Ok(());
        }

        let dir = self.build_dir(ctx);
        println!("configuring/building/installing vulkan-loader in {}", dir.display());
        cmake_build_and_install(
            ctx,
            &dir,
            &[
                // No X11/Wayland-specific surface extensions beyond what
                // this workspace's own Wayland-only stack (§6.6, §10.3.3)
                // needs — no XCB (§9's deliberate gap), no Xlib.
                "-DBUILD_WSI_XCB_SUPPORT=OFF",
                "-DBUILD_WSI_XLIB_SUPPORT=OFF",
                "-DBUILD_WSI_WAYLAND_SUPPORT=ON",
                "-DBUILD_TESTS=OFF",
                "-DUPDATE_DEPS=OFF",
            ],
        )
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "libvulkan.so.1 (sysroot marker)".to_string(),
            path: ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/libvulkan.so.1"),
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
