//! procps-ng (`ps`, `pgrep`, `pkill`, `free`, `uptime`, ...) — like
//! `grep`/`sed`/`find`, never part of coreutils; see `grep.rs`'s doc
//! comment for the full story. `top`/`watch` (the only ncurses-dependent
//! binaries) are skipped via `--without-ncurses`, keeping this project's
//! dependency footprint unchanged.

use anyhow::Context;
use buildpack_core::build::autotools_build_and_install;
use buildpack_core::run::{already_built, run_in};
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source, SourcePatch};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ProcpsConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Procps {
    cfg: ProcpsConfig,
}

impl Procps {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("procps-v{}", self.cfg.version))
    }
}

impl Buildpack for Procps {
    fn id(&self) -> &'static str {
        "procps"
    }

    fn configure(&mut self, table: &toml::Value) -> anyhow::Result<()> {
        self.cfg = table.clone().try_into().context("parsing [procps] config")?;
        Ok(())
    }

    fn to_toml(&self) -> anyhow::Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [procps] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    fn describe(&self) -> Description {
        Description {
            id: "procps",
            name: "procps-ng",
            summary: "ps, pgrep, pkill, free, uptime — never part of coreutils",
            long_description: "Autotools build (autogen.sh regenerates it — the upstream \
                GitLab source archive ships no pre-built configure script, unlike GNU's own \
                dist tarballs). ncurses (top/watch), systemd, and NLS all disabled — only the \
                plain process/status tools this sysroot otherwise has no equivalent for.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("procps-v{}.tar.gz", self.cfg.version),
            extracted_dir_name: format!("procps-v{}", self.cfg.version),
        }]
    }

    fn patches(&self, _ctx: &BuildCtx) -> Vec<SourcePatch> {
        vec![SourcePatch {
            description: "run autogen.sh to generate configure (no dist tarball upstream)",
            apply: run_autogen,
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> anyhow::Result<()> {
        let marker = ctx.sysroot_dir.join("usr/bin/ps");
        if already_built(&marker, force) {
            println!("skip build-procps: {} already exists", marker.display());
            return Ok(());
        }

        let dir = self.build_dir(ctx);
        println!("configuring/building/installing procps in {}", dir.display());
        autotools_build_and_install(ctx, &dir, &["--disable-nls", "--without-ncurses", "--with-systemd=no"])
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![
            BuildOutput {
                description: "ps binary (sysroot marker)".to_string(),
                path: ctx.sysroot_dir.join("usr/bin/ps"),
                rootfs_install: None,
            },
            BuildOutput {
                description: "pgrep binary".to_string(),
                path: ctx.sysroot_dir.join("usr/bin/pgrep"),
                rootfs_install: None,
            },
            BuildOutput {
                description: "pkill binary".to_string(),
                path: ctx.sysroot_dir.join("usr/bin/pkill"),
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

fn run_autogen(dir: &std::path::Path) -> anyhow::Result<()> {
    if dir.join("configure").exists() {
        println!("procps configure already generated");
        return Ok(());
    }
    println!("running procps autogen.sh (needs autopoint from the host's gettext package)");
    run_in(dir, &mut Command::new("./autogen.sh"))
}
