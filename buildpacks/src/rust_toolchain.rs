//! A working Rust toolchain (`rustc` + `cargo`) *on the target image* —
//! distinct from every other use of Rust in this project so far, which
//! is the host's own toolchain compiling buildpacks like `distro-init`.
//! §11's own "What Isn't Part of the Picture Yet" named this the
//! natural next phase once the image could actually boot into a usable
//! terminal: once `cargo` exists on-target, a curated Rust CLI-tools
//! suite (ripgrep, bat, eza, ...) becomes trivial to add later.
//!
//! Building `rustc` from source is a multi-hour, multi-stage bootstrap
//! process — not practical here, and not how this project treats
//! similar cases. Same category as glibc's own shared libraries
//! (`HOST_DYNAMIC_LIBS`) and the locale/Compose data
//! (`install_locale_data`), both in `distro/src/stages/rootfs.rs`: when
//! something is realistically only available as a pre-built binary
//! distribution, use the *official* one. Unlike those two, though, this
//! genuinely goes through the normal buildpack pipeline (fetched,
//! versioned, installed via `rust-installer`'s own `install.sh`) rather
//! than being copied straight from the host — there's no "host's own
//! Rust toolchain for musl" equivalent to lean on, and pinning an exact
//! upstream release is the right thing regardless.
//!
//! `distro` (glibc) only. Official Rust has no native musl-hosted
//! `rustc` at all — only `cargo` ships an `x86_64-unknown-linux-musl`
//! host build; `rust-std` for musl exists solely as a cross-compilation
//! target, not something `rustc` itself can run on. Getting Rust running
//! natively on `distroless` would mean bootstrapping `rustc` from source
//! targeting musl as host, or pulling from an unofficial third-party
//! distribution instead of upstream — both out of scope here.
//!
//! `rustc`/`cargo` alone only get you as far as `rustc --version` and
//! dependency resolution — actually linking a binary needs `cc` as a
//! driver too, confirmed still true (and not changing soon) even with
//! this tarball's own bundled `rust-lld`; see `native_gcc.rs`'s own doc
//! comment for why that's a separate buildpack, not folded in here.

use anyhow::{Context, Result};
use buildpack_core::build::sysroot_env;
use buildpack_core::run::{already_built, run_in};
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RustToolchainConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct RustToolchain {
    cfg: RustToolchainConfig,
}

impl RustToolchain {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("rust-{}-x86_64-unknown-linux-gnu", self.cfg.version))
    }
}

impl Buildpack for RustToolchain {
    fn id(&self) -> &'static str {
        "rust_toolchain"
    }

    fn configure(&mut self, table: &toml::Value) -> Result<()> {
        self.cfg = table.clone().try_into().context("parsing [rust_toolchain] config")?;
        Ok(())
    }

    fn to_toml(&self) -> Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [rust_toolchain] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &["native_gcc"]
    }

    fn describe(&self) -> Description {
        Description {
            id: "rust_toolchain",
            name: "Rust toolchain",
            summary: "rustc + cargo, on the target image itself (not host build tooling)",
            long_description: "Official prebuilt binary distribution from static.rust-lang.org, \
                installed via rust-installer's own install.sh — not built from source (rustc's \
                own bootstrap is a multi-hour, multi-stage process). rust-docs and the JSON docs \
                preview both excluded; nothing on this headless-ish image will browse them.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("rust-{}-x86_64-unknown-linux-gnu.tar.gz", self.cfg.version),
            extracted_dir_name: format!("rust-{}-x86_64-unknown-linux-gnu", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> Result<()> {
        let marker = ctx.sysroot_dir.join("usr/bin/rustc");
        if already_built(&marker, force) {
            println!("skip build-rust-toolchain: {} already exists", marker.display());
            return Ok(());
        }

        let dir = self.build_dir(ctx);
        let sysroot_abs = std::env::current_dir()
            .context("getting current directory")?
            .join(&ctx.sysroot_dir);

        println!("installing rust toolchain from {} into {}", dir.display(), sysroot_abs.display());
        let mut cmd = Command::new("./install.sh");
        cmd.args([
            "--prefix=/usr",
            &format!("--destdir={}", sysroot_abs.display()),
            "--disable-ldconfig",
            "--without=rust-docs,rust-docs-json-preview",
        ]);
        sysroot_env(ctx, &mut cmd)?;
        run_in(&dir, &mut cmd)
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![
            BuildOutput {
                description: "rustc binary (sysroot marker)".to_string(),
                path: ctx.sysroot_dir.join("usr/bin/rustc"),
                rootfs_install: None,
            },
            BuildOutput {
                description: "cargo binary".to_string(),
                path: ctx.sysroot_dir.join("usr/bin/cargo"),
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
