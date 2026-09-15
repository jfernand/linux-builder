//! `cosmic-comp` — the COSMIC desktop's own Wayland compositor, built on
//! `smithay`. The first of COSMIC's ~28 real components (§11 of the
//! report; `pop-os/cosmic-epoch`'s actual submodule list, not a guess) —
//! everything else in a minimal session depends on the compositor
//! existing first.
//!
//! Verified via a real `cargo check` against this workspace's existing
//! sysroot before this buildpack was written: every system dependency
//! `smithay`'s feature set needs (`libseat`, `libinput`, `libdrm`+`gbm`,
//! `libudev`, `libxkbcommon`, `wayland-client`/`-server`) is already
//! satisfied by buildpacks this project already has — no new C library
//! buildpacks needed for the compositor itself. `zbus` (D-Bus) is a
//! pure-Rust wire-protocol implementation, not a `libdbus` binding, so it
//! needs no C dependency either.
//!
//! One real upstream issue found and patched: `Cargo.toml`'s own
//! `[patch."https://github.com/pop-os/cosmic-protocols"]` section names
//! its replacement source with a literal doubled slash
//! (`pop-os//cosmic-protocols`) — likely load-bearing upstream only
//! because Cargo's git-source dedup treats it as a distinct string from
//! the correctly-spelled main dependency, letting the patch silently take
//! priority. Fixing the typo makes Cargo correctly reject the patch as
//! redundant (same source, different ref) instead — so `patches()` fixes
//! the typo *and* removes the now-redundant patch block, retargeting the
//! main dependency to the same `branch = "main"` ref the patch and the
//! committed `Cargo.lock` already resolve to.
//!
//! Vulkan is a known, deliberate gap, not an oversight: `smithay`'s
//! `backend_vulkan` feature compiles in fine (the `ash` crate needs no
//! Vulkan SDK at build time), but this project's own Mesa build has
//! Vulkan drivers disabled (`-Dvulkan-drivers=`, empty — §10.3.4).
//! `cosmic-comp` should fall back to `renderer_glow` (GL via EGL, which
//! this workspace's Mesa build does provide) at runtime, same as Weston's
//! own `softpipe` fallback — not yet verified with a real boot.

use anyhow::{bail, Context, Result};
use buildpack_core::build::{cargo_build_release_sysroot, cargo_target_dir};
use buildpack_core::run::already_built;
use buildpack_core::{
    BuildCtx, BuildOutput, Buildpack, Description, InstallMode, RootfsInstall, Source, SourcePatch,
};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct CosmicCompConfig {
    pub git_url: String,
    pub git_rev: String,
}

#[derive(Default)]
pub struct CosmicComp {
    cfg: CosmicCompConfig,
}

impl CosmicComp {
    pub fn new() -> Self {
        Self::default()
    }

    fn checkout_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join("cosmic-comp")
    }

    fn binary_path(&self, ctx: &BuildCtx) -> PathBuf {
        cargo_target_dir(&self.checkout_dir(ctx)).join("release").join("cosmic-comp")
    }
}

impl Buildpack for CosmicComp {
    fn id(&self) -> &'static str {
        "cosmic_comp"
    }

    fn configure(&mut self, table: &toml::Value) -> Result<()> {
        self.cfg = table.clone().try_into().context("parsing [cosmic_comp] config")?;
        Ok(())
    }

    fn to_toml(&self) -> Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [cosmic_comp] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &["seatd", "eudev", "libinput", "libdrm", "mesa", "libxkbcommon", "wayland"]
    }

    fn describe(&self) -> Description {
        Description {
            id: "cosmic_comp",
            name: "cosmic-comp",
            summary: "The COSMIC desktop's Wayland compositor, built on smithay",
            long_description: "Native cargo build against this workspace's existing sysroot — \
                no new C library dependencies beyond what Weston's own chain already built. \
                One upstream Cargo.toml patch: a doubled-slash git-patch source typo.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Git {
            url: self.cfg.git_url.clone(),
            rev: self.cfg.git_rev.clone(),
            checkout_dir_name: "cosmic-comp".to_string(),
        }]
    }

    fn patches(&self, _ctx: &BuildCtx) -> Vec<SourcePatch> {
        vec![SourcePatch {
            description: "fix the doubled-slash cosmic-protocols patch source, and drop the now-redundant [patch] block",
            apply: patch_cargo_toml,
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> Result<()> {
        let binary = self.binary_path(ctx);
        if already_built(&binary, force) {
            println!("skip build-cosmic-comp: {} already exists", binary.display());
            return Ok(());
        }

        let dir = self.checkout_dir(ctx);
        println!("building cosmic-comp in {}", dir.display());
        cargo_build_release_sysroot(
            ctx,
            &dir,
            &[],
            &[
                // rust-toolchain.toml pins an exact 1.93 that isn't
                // installed here; the workspace's own default stable
                // toolchain is newer and satisfies Cargo.toml's
                // rust-version = "1.93" minimum.
                ("RUSTUP_TOOLCHAIN", "stable"),
                // cargo's built-in libgit2 fetcher rejects one upstream
                // git dependency URL outright (see the module doc); the
                // system git CLI handles it fine.
                ("CARGO_NET_GIT_FETCH_WITH_CLI", "true"),
            ],
        )
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "cosmic-comp binary".to_string(),
            path: self.binary_path(ctx),
            rootfs_install: Some(RootfsInstall {
                dest: PathBuf::from("usr/bin/cosmic-comp"),
                symlinks: Vec::new(),
            }),
        }]
    }

    fn install_mode(&self) -> InstallMode {
        InstallMode::StaticArtifacts
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn patch_cargo_toml(dir: &std::path::Path) -> Result<()> {
    let path = dir.join("Cargo.toml");
    let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;

    const ALREADY_PATCHED: &str =
        "cosmic-protocols = { git = \"https://github.com/pop-os/cosmic-protocols\", branch = \"main\", default-features = false, features = [";
    if text.contains(ALREADY_PATCHED) {
        println!("cosmic-comp Cargo.toml already patched");
        return Ok(());
    }

    const OLD_DEP: &str = "cosmic-protocols = { git = \"https://github.com/pop-os/cosmic-protocols\", rev = \"160b086\", default-features = false, features = [";
    const NEW_DEP: &str = "cosmic-protocols = { git = \"https://github.com/pop-os/cosmic-protocols\", branch = \"main\", default-features = false, features = [";

    const OLD_PATCH_BLOCK: &str = "[patch.\"https://github.com/pop-os/cosmic-protocols\"]\ncosmic-protocols = { git = \"https://github.com/pop-os//cosmic-protocols\", branch = \"main\" }\n\n";

    if !text.contains(OLD_DEP) || !text.contains(OLD_PATCH_BLOCK) {
        bail!(
            "couldn't find the expected cosmic-protocols dependency/patch text in {} \
             (upstream may have changed it)",
            path.display()
        );
    }

    println!("patching cosmic-comp Cargo.toml: cosmic-protocols branch=main, dropping the redundant doubled-slash [patch] block");
    let text = text.replace(OLD_DEP, NEW_DEP).replace(OLD_PATCH_BLOCK, "");
    std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))
}
