use anyhow::Result;
use buildpack_core::config::DistroConfig;
use buildpacks::kernel::KernelChannel;
use std::path::Path;

/// `distro`'s own wrapper around `buildpacks::kernel::latest_release` —
/// patches just the `[kernel]` table's `version`/`url` fields via
/// `DistroConfig`, rather than a whole-`Config` round-trip through typed
/// per-package structs `distro` no longer has (see `DistroConfig`'s
/// untyped `packages` bag).
pub fn resolve_kernel(config_path: &Path, channel: KernelChannel) -> Result<()> {
    let (version, url) = buildpacks::kernel::latest_release(channel)?;

    let mut cfg = DistroConfig::load(config_path)?;
    let mut kernel = cfg.package_table("kernel");
    let table = kernel.as_table_mut().expect("[kernel] is a table");
    table.insert("version".to_string(), toml::Value::String(version));
    table.insert("url".to_string(), toml::Value::String(url));
    cfg.packages.insert("kernel".to_string(), kernel);
    cfg.save(config_path)?;

    println!("wrote kernel.version/url to {}", config_path.display());
    println!(
        "if you already fetched a different version's sources, remove the old \
         kernel source directory and re-run `fetch` (distro's fetch has no --clean flag)"
    );
    Ok(())
}
