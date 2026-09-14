use crate::config::Config;
use anyhow::Result;
use builder_core::stages::KernelChannel;
use std::path::Path;

/// `distro`'s own wrapper around `builder_core::stages::kernel::latest_release`
/// — unlike `distroless`, `distro` can't call `builder_core::stages::kernel::resolve_kernel`
/// directly: that function loads and saves a whole `builder_core::config::Config`,
/// which doesn't have `distro`'s `bash`/`util_linux`/`shadow`/`seatd`/`dbus`/…
/// sections, so a save would silently drop them. This loads/mutates/saves
/// `distro`'s own `Config` instead, reusing only the generic kernel.org
/// lookup.
pub fn resolve_kernel(config_path: &Path, channel: KernelChannel) -> Result<()> {
    let (version, url) = builder_core::stages::kernel::latest_release(channel)?;

    let mut cfg = Config::load(config_path)?;
    cfg.kernel.version = version;
    cfg.kernel.url = url;
    cfg.save(config_path)?;

    println!("wrote kernel.version/url to {}", config_path.display());
    println!(
        "if you already fetched a different version's sources, remove the old \
         kernel source directory and re-run `fetch` (distro's fetch has no --clean flag)"
    );
    Ok(())
}
