//! `distro-init` — our own from-scratch PID 1, built in place from
//! source already in this repo (a workspace member), not fetched.

use anyhow::Context;
use buildpack_core::build::cargo_target_dir;
use buildpack_core::run::{already_built, run_in};
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, RootfsInstall, Source};
use std::any::Any;
use std::path::PathBuf;
use std::process::Command;

const GNU_TARGET: &str = "x86_64-unknown-linux-gnu";

#[derive(Default)]
pub struct DistroInit;

impl DistroInit {
    pub fn new() -> Self {
        Self::default()
    }

    /// Not under `ctx.sources_dir` like fetched sources — a workspace
    /// `target/` path, since it's built from source already in this
    /// repo. Honors `CARGO_TARGET_DIR` the same way cargo itself does.
    fn binary_path(&self) -> PathBuf {
        cargo_target_dir(&PathBuf::from(".")).join(GNU_TARGET).join("release").join("distro-init")
    }
}

impl Buildpack for DistroInit {
    fn id(&self) -> &'static str {
        "distro_init"
    }

    fn configure(&mut self, _table: &toml::Value) -> anyhow::Result<()> {
        Ok(())
    }

    fn to_toml(&self) -> anyhow::Result<toml::Value> {
        Ok(toml::Value::Table(Default::default()))
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    fn required(&self) -> bool {
        true // PID 1 — nothing boots without it
    }

    fn describe(&self) -> Description {
        Description {
            id: "distro_init",
            name: "distro-init",
            summary: "Our own PID 1",
            long_description: "Built in place from this repo's own distro-init workspace \
                member, not fetched — no source/patch step.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::InTree]
    }

    fn build(&self, _ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let binary = self.binary_path();
        if already_built(&binary, force) {
            println!("skip build-init: {} already exists", binary.display());
            return Ok(());
        }

        // Assumes distro is invoked from the repo root, same as
        // distro.toml's own relative paths already do.
        let workspace_root = std::env::current_dir().context("getting current directory")?;

        println!("building distro-init (static)");
        run_in(
            &workspace_root,
            Command::new("cargo")
                .args(["build", "--release", "-p", "distro-init", "--target", GNU_TARGET])
                .env("RUSTFLAGS", "-C target-feature=+crt-static"),
        )
    }

    fn outputs(&self, _ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "distro-init binary".to_string(),
            path: self.binary_path(),
            rootfs_install: Some(RootfsInstall { dest: PathBuf::from("sbin/init"), symlinks: vec![] }),
        }]
    }

    fn install_mode(&self) -> InstallMode {
        InstallMode::StaticArtifacts
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
