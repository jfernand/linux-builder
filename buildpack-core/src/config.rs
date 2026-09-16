//! The shared config container both `distro` and `distroless` load their
//! `distro.toml`/`distroless.toml` through. Replaces `builder_core::
//! config::Config` and `distro`'s own hand-rolled `Config` — neither of
//! which needs to understand a package's config shape (see
//! `buildpack-core`'s plan file, Phase 2): every package's table is kept
//! as a raw `toml::Value` here and only round-tripped through that
//! package's own `Buildpack::configure`/`to_toml`.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ImageSettings {
    pub arch: String,
    pub size_mb: u64,
    pub hostname: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct BuildSettings {
    /// Ids of "final" buildpacks (§11's sense: nothing else's
    /// `dependencies()` lists them, so build-order graph-theoretically
    /// they're leaves — a real compositor, a shell, an end-user binary,
    /// not a shared library) to skip entirely. `all_packages()` prunes
    /// each one along with anything whose *only* remaining path forward
    /// led exclusively to it — see `graph::prune_disabled`.
    #[serde(default)]
    pub disabled: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DistroConfig {
    pub build_dir: PathBuf,
    pub networking: bool,
    pub image: ImageSettings,
    pub build: BuildSettings,
    /// Every other top-level table in the TOML file, keyed by section
    /// name — one entry per buildpack id. Untyped on purpose: this
    /// container never needs to know what's inside a package's table,
    /// only that it exists to hand to that buildpack's `configure()`.
    pub packages: BTreeMap<String, toml::Value>,
}

/// The 4 keys this container itself understands; everything else in the
/// file is a package table.
const RESERVED_KEYS: &[&str] = &["build_dir", "networking", "image", "build"];

impl DistroConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config file {}", path.display()))?;
        let root: toml::Value = toml::from_str(&text)
            .with_context(|| format!("parsing config file {}", path.display()))?;
        let table = root
            .as_table()
            .with_context(|| format!("{} is not a TOML table at the top level", path.display()))?;

        let build_dir: PathBuf = table
            .get("build_dir")
            .cloned()
            .context("missing build_dir")?
            .try_into()
            .context("parsing build_dir")?;
        let networking = table
            .get("networking")
            .cloned()
            .map(|v| v.try_into())
            .transpose()
            .context("parsing networking")?
            .unwrap_or(false);
        let image = table
            .get("image")
            .cloned()
            .context("missing [image] section")?
            .try_into()
            .context("parsing [image]")?;

        let build = table
            .get("build")
            .cloned()
            .map(|v| v.try_into())
            .transpose()
            .context("parsing [build]")?
            .unwrap_or_default();

        let packages = table
            .iter()
            .filter(|(k, _)| !RESERVED_KEYS.contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        Ok(Self { build_dir, networking, image, build, packages })
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let mut table = toml::map::Map::new();
        table.insert("build_dir".to_string(), toml::Value::try_from(&self.build_dir)?);
        table.insert("networking".to_string(), toml::Value::Boolean(self.networking));
        table.insert("image".to_string(), toml::Value::try_from(&self.image)?);
        if !self.build.disabled.is_empty() {
            table.insert("build".to_string(), toml::Value::try_from(&self.build)?);
        }
        for (id, value) in &self.packages {
            table.insert(id.clone(), value.clone());
        }
        let text = toml::to_string_pretty(&toml::Value::Table(table)).context("serializing config")?;
        std::fs::write(path, text)
            .with_context(|| format!("writing config file {}", path.display()))
    }

    /// The `[<id>]` table for one package, or an empty table if the
    /// config file has no section for it — the convention every
    /// `Buildpack::configure` already expects.
    pub fn package_table(&self, id: &str) -> toml::Value {
        self.packages.get(id).cloned().unwrap_or_else(|| toml::Value::Table(Default::default()))
    }

    pub fn sources_dir(&self) -> PathBuf {
        self.build_dir.join("sources")
    }

    pub fn sysroot_dir(&self) -> PathBuf {
        self.build_dir.join("sysroot")
    }

    pub fn rootfs_dir(&self) -> PathBuf {
        self.build_dir.join("rootfs")
    }

    pub fn output_image(&self) -> PathBuf {
        self.build_dir.join("output.img")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_distro_toml() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../distro.toml");
        let cfg = DistroConfig::load(&path).expect("load distro.toml");
        assert_eq!(cfg.build_dir, PathBuf::from("build-distro"));
        assert_eq!(cfg.image.hostname, "distro");
        assert_eq!(cfg.image.size_mb, 2048);
        assert_eq!(cfg.image.arch, "x86_64");
        assert_eq!(cfg.packages.len(), 32); // every [section] except build_dir/networking/image
        let mesa = cfg.packages.get("mesa").expect("mesa section");
        assert_eq!(mesa.get("version").and_then(|v| v.as_str()), Some("26.2.2"));

        let out = std::env::temp_dir().join("distro-toml-roundtrip-test.toml");
        cfg.save(&out).expect("save");
        let reloaded = DistroConfig::load(&out).expect("reload");
        assert_eq!(reloaded.image.hostname, cfg.image.hostname);
        assert_eq!(reloaded.packages.get("mesa"), cfg.packages.get("mesa"));
        std::fs::remove_file(&out).ok();
    }
}
