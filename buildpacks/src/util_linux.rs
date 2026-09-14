//! The util-linux buildpack — autotools, `InstallMode::StaticArtifacts`,
//! and a real config-driven branch (`full`) inside both `build()` and
//! `outputs()`. Ported from `distro/src/stages/userland.rs`'s
//! `build_util_linux` and `distro/src/stages/rootfs.rs`'s
//! `install_util_linux`/`install_util_linux_full`.

use anyhow::{Context, Result};
use buildpack_core::build::autotools_build_static;
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, RootfsInstall, Source};
use serde::Deserialize;
use std::any::Any;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UtilLinuxConfig {
    pub version: String,
    pub url: String,
    /// By default only agetty/mount/umount are built (all Phase 1 needs).
    /// Set true to build the rest of util-linux's ~120 programs too —
    /// off by default to keep the minimal-base philosophy, opt-in the
    /// same way the kernel's FEATURE_PACKS are.
    #[serde(default)]
    pub full: bool,
}

#[derive(Default)]
pub struct UtilLinux {
    cfg: UtilLinuxConfig,
}

impl UtilLinux {
    pub fn new() -> Self {
        Self::default()
    }

    /// Matches `default_fetch`'s extraction target — see `Kernel::build_dir`'s
    /// doc comment for why these must agree.
    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("util-linux-{}", self.cfg.version))
    }
}

impl Buildpack for UtilLinux {
    fn id(&self) -> &'static str {
        "util_linux"
    }

    fn configure(&mut self, table: &toml::Value) -> Result<()> {
        self.cfg = table.clone().try_into().context("parsing [util_linux] config")?;
        Ok(())
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    fn describe(&self) -> Description {
        Description {
            id: "util_linux",
            name: "util-linux",
            summary: "agetty/mount/umount, or (with `full`) all ~120 of its programs",
            long_description: "Built statically via autotools. Off by default, only \
                agetty/mount/umount are built (all Phase 1 needs); set `full = true` \
                to build the rest of util-linux's programs too.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("util-linux-{}.tar.xz", self.cfg.version),
            extracted_dir_name: format!("util-linux-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> Result<()> {
        let dir = self.build_dir(ctx);
        let marker = dir.join("agetty");

        if already_built(&marker, force) {
            println!("skip build-util-linux: {} already exists", marker.display());
            return Ok(());
        }

        let configure_args: &[&str] = if self.cfg.full {
            &[
                "--enable-all-programs",
                // sqlite3's static .a isn't linked against -lm on this host,
                // breaking lastlog2's static link; udev's static .a doesn't
                // exist at all here (shared-only), breaking findmnt/lsblk;
                // pylibmount is a shared-only libtool module, incompatible
                // with --disable-shared; ncursesw/ncurses/slang's static
                // libs have their own unrelated static-link gaps on this
                // host, breaking cfdisk (and, as a side effect of no
                // curses UI library at all, irqtop/ul/more/pg/setterm too)
                // — all found by actually trying the full static build,
                // not guessed at.
                "--disable-liblastlog2",
                "--without-udev",
                "--without-python",
                "--without-ncursesw",
                "--without-ncurses",
                "--without-slang",
                "--disable-shared",
                "--enable-static",
            ]
        } else {
            &[
                "--disable-all-programs",
                "--enable-agetty",
                "--enable-mount",
                "--enable-libmount",
                "--enable-libblkid",
                "--enable-libuuid",
                "--disable-shared",
                "--enable-static",
            ]
        };

        println!(
            "configuring util-linux (static, {}) in {}",
            if self.cfg.full { "full" } else { "agetty+mount only" },
            dir.display()
        );
        autotools_build_static(&dir, configure_args, &[("LDFLAGS", "-all-static")])
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        let dir = self.build_dir(ctx);

        if !self.cfg.full {
            return [("agetty", "sbin"), ("mount", "bin"), ("umount", "bin")]
                .into_iter()
                .map(|(name, dest_dir)| BuildOutput {
                    description: "util-linux binary",
                    path: dir.join(name),
                    rootfs_install: Some(RootfsInstall {
                        dest: PathBuf::from(dest_dir).join(name),
                        symlinks: Vec::new(),
                    }),
                })
                .collect();
        }

        // util-linux's autotools build places every program flat in the
        // top level of the build directory regardless of which source
        // subdir (sys-utils/, disk-utils/, ...) it came from. Filtered by
        // ELF magic bytes rather than the executable bit alone, since
        // that same top level also holds executable shell scripts
        // (configure, config.status, libtool) that aren't programs to ship.
        let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
        entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file() && is_executable(p) && is_elf(p).unwrap_or(false))
            .map(|p| {
                let name = p.file_name().unwrap().to_string_lossy().into_owned();
                let dest_dir = if name == "agetty" { "sbin" } else { "bin" };
                BuildOutput {
                    description: "util-linux binary",
                    path: p,
                    rootfs_install: Some(RootfsInstall {
                        dest: PathBuf::from(dest_dir).join(&name),
                        symlinks: Vec::new(),
                    }),
                }
            })
            .collect()
    }

    fn install_mode(&self) -> InstallMode {
        InstallMode::StaticArtifacts
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path).map(|m| m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

fn is_elf(path: &Path) -> Result<bool> {
    let mut buf = [0u8; 4];
    let mut f = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    Ok(f.read(&mut buf)? == 4 && buf == *b"\x7fELF")
}
