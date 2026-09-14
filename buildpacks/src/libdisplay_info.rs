//! EDID/DisplayID parsing — how a compositor reads a monitor's own
//! description of its supported modes.

use anyhow::{bail, Context, Result};
use buildpack_core::build::meson_build_and_install;
use buildpack_core::run::already_built;
use buildpack_core::{
    BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source, SourcePatch,
};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct LibdisplayInfoConfig {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct LibdisplayInfo {
    cfg: LibdisplayInfoConfig,
}

impl LibdisplayInfo {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_dir(&self, ctx: &BuildCtx) -> PathBuf {
        ctx.sources_dir.join(format!("libdisplay-info-{}", self.cfg.version))
    }
}

impl Buildpack for LibdisplayInfo {
    fn id(&self) -> &'static str {
        "libdisplay_info"
    }

    fn configure(&mut self, table: &toml::Value) -> Result<()> {
        self.cfg = table.clone().try_into().context("parsing [libdisplay_info] config")?;
        Ok(())
    }

    fn to_toml(&self) -> Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [libdisplay_info] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    fn describe(&self) -> Description {
        Description {
            id: "libdisplay_info",
            name: "libdisplay-info",
            summary: "EDID/DisplayID parsing",
            long_description: "Meson build, no options. Patches its own hwdata lookup — see \
                patches().",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::Tarball {
            url: self.cfg.url.clone(),
            archive_name: format!("libdisplay-info-{}.tar.xz", self.cfg.version),
            extracted_dir_name: format!("libdisplay-info-{}", self.cfg.version),
        }]
    }

    fn patches(&self, _ctx: &BuildCtx) -> Vec<SourcePatch> {
        vec![SourcePatch {
            description: "force the literal hwdata fallback path in meson.build",
            apply: patch_hwdata,
        }]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> Result<()> {
        let marker = ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/libdisplay-info.pc");
        if already_built(&marker, force) {
            println!("skip build-libdisplay-info: {} already exists", marker.display());
            return Ok(());
        }

        let dir = self.build_dir(ctx);
        println!("configuring/building/installing libdisplay-info in {}", dir.display());
        meson_build_and_install(ctx, &dir, &[])
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "libdisplay-info.pc (sysroot marker)".to_string(),
            path: ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu/pkgconfig/libdisplay-info.pc"),
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

/// libdisplay-info's meson.build looks up `hwdata` (the package providing
/// `/usr/share/hwdata/pnp.ids`, a vendor-ID database it embeds into the
/// built library at compile time — a build-time-only need, nothing reads
/// it at target runtime) via `dependency('hwdata', ...).get_variable(...)`.
/// That variable resolves correctly on the *host* (where hwdata is
/// actually installed), but `PKG_CONFIG_SYSROOT_DIR` — necessary for
/// every package that genuinely does live under our sysroot — rewrites it
/// into a sysroot path hwdata was never installed under, since hwdata is
/// found via pkg-config's own default search, not the sysroot's
/// PKG_CONFIG_PATH. libdisplay-info's own meson.build already has an
/// unconditional fallback to the literal, correct host path in its `else`
/// branch — this patch just always takes it.
fn patch_hwdata(dir: &Path) -> Result<()> {
    let path = dir.join("meson.build");
    let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;

    if !text.contains("dep_hwdata = dependency(") {
        println!("libdisplay-info hwdata lookup already patched");
        return Ok(());
    }

    const OLD: &str = "dep_hwdata = dependency('hwdata', required: false, native: true)\nif dep_hwdata.found()\n\thwdata_dir = dep_hwdata.get_variable(pkgconfig: 'pkgdatadir')\n\tpnp_ids = files(hwdata_dir / 'pnp.ids')\nelse\n\tpnp_ids = files('/usr/share/hwdata/pnp.ids')\nendif";
    const NEW: &str = "pnp_ids = files('/usr/share/hwdata/pnp.ids')";

    if !text.contains(OLD) {
        bail!(
            "couldn't find the expected hwdata lookup code in {} to patch (upstream may have changed it)",
            path.display()
        );
    }

    println!("patching libdisplay-info to always use the literal hwdata path");
    std::fs::write(&path, text.replace(OLD, NEW)).with_context(|| format!("writing {}", path.display()))
}
