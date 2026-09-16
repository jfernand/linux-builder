//! DejaVu — pure data (TrueType font files + fontconfig alias snippets,
//! no C code, no shared library), the same category as `xkeyboard_config`.
//! Its absence is exactly what made Alacritty fail with `Font(FontNotFound
//! ...))` in a real QEMU boot test: `fontconfig` (the library) has been
//! built and configured (§10.3.5's table) since the Weston chain, but
//! nothing has ever shipped an actual font *file* for it to resolve
//! "monospace"/"sans-serif"/"serif" to.
//!
//! Distributed upstream only as pre-built release tarballs (`.ttf` files),
//! not buildable from source without FontForge (a GUI font editor,
//! Python-scriptable) — the same "data asset, not compiled software"
//! reasoning that makes this not a Rust/C-only rule concern, matching how
//! this project already treats `xkeyboard_config`'s XML/lua data.

use anyhow::{Context, Result};
use buildpack_core::run::already_built;
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DejavuFontsConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct DejavuFonts {
    cfg: DejavuFontsConfig,
}

impl DejavuFonts {
    pub fn new() -> Self {
        Self::default()
    }

    fn extracted_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("dejavu-fonts-ttf-{}", self.cfg.version))
    }

    fn marker(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sysroot_dir.join("usr/share/fonts/dejavu/DejaVuSansMono.ttf")
    }
}

impl Buildpack for DejavuFonts {
    fn id(&self) -> &'static str {
        "dejavu_fonts"
    }

    fn configure(&mut self, table: &toml::Value) -> Result<()> {
        self.cfg = table.clone().try_into().context("parsing [dejavu_fonts] config")?;
        Ok(())
    }

    fn to_toml(&self) -> Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [dejavu_fonts] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    fn describe(&self) -> Description {
        Description {
            id: "dejavu_fonts",
            name: "DejaVu Fonts",
            summary: "A real monospace/sans/serif font — fontconfig has nothing to resolve without it",
            long_description: "Pure data package (pre-built TrueType files + fontconfig alias \
                snippets), no C code. Installs to /usr/share/fonts/dejavu (already scanned by \
                this sysroot's own /etc/fonts/fonts.conf) and /etc/fonts/conf.d (the generic \
                family aliases fontconfig's default config already loads).",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("dejavu-fonts-ttf-{}.tar.bz2", self.cfg.version),
            extracted_dir_name: format!("dejavu-fonts-ttf-{}", self.cfg.version),
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> Result<()> {
        let marker = self.marker(ctx);
        if already_built(&marker, force) {
            println!("skip build-dejavu-fonts: {} already exists", marker.display());
            return Ok(());
        }

        let dir = self.extracted_dir(ctx);
        println!("installing dejavu-fonts from {}", dir.display());

        let fonts_dest = ctx.sysroot_dir.join("usr/share/fonts/dejavu");
        std::fs::create_dir_all(&fonts_dest).context("creating usr/share/fonts/dejavu")?;
        for entry in std::fs::read_dir(dir.join("ttf")).context("reading ttf/ dir")? {
            let entry = entry?;
            if entry.path().extension().is_some_and(|e| e == "ttf") {
                std::fs::copy(entry.path(), fonts_dest.join(entry.file_name()))
                    .with_context(|| format!("copying {}", entry.path().display()))?;
            }
        }

        let conf_dest = ctx.sysroot_dir.join("etc/fonts/conf.d");
        std::fs::create_dir_all(&conf_dest).context("creating etc/fonts/conf.d")?;
        for entry in std::fs::read_dir(dir.join("fontconfig")).context("reading fontconfig/ dir")? {
            let entry = entry?;
            if entry.path().extension().is_some_and(|e| e == "conf") {
                std::fs::copy(entry.path(), conf_dest.join(entry.file_name()))
                    .with_context(|| format!("copying {}", entry.path().display()))?;
            }
        }

        Ok(())
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "DejaVuSansMono.ttf (sysroot marker)".to_string(),
            path: self.marker(ctx),
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
