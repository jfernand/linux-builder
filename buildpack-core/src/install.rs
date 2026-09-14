//! Generic rootfs installation for a `StaticArtifacts` buildpack's
//! declared `BuildOutput`s — copies `dest` in, then symlinks every entry
//! in `symlinks` to it with a correctly computed *relative* target (not
//! just the bare file name), so a symlink in a different directory than
//! its target (e.g. `sbin/init -> ../bin/busybox`) works the same as one
//! in the same directory (e.g. `bin/sh -> bash`).

use crate::BuildOutput;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub fn install_output(root: &Path, out: &BuildOutput) -> Result<()> {
    let Some(install) = &out.rootfs_install else { return Ok(()) };

    let dest = root.join(&install.dest);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::copy(&out.path, &dest)
        .with_context(|| format!("copying {} to {}", out.path.display(), dest.display()))?;

    for link in &install.symlinks {
        let link_path = root.join(link);
        if let Some(parent) = link_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let target = relative_target(&install.dest, link);
        let _ = std::fs::remove_file(&link_path);
        std::os::unix::fs::symlink(&target, &link_path)
            .with_context(|| format!("symlinking {} -> {}", link_path.display(), target.display()))?;
    }

    Ok(())
}

/// The relative path from `link`'s directory to `dest`, both given as
/// rootfs-relative paths (e.g. `dest = "bin/busybox"`, `link =
/// "sbin/init"` -> `"../bin/busybox"`; `dest = "bin/bash"`, `link =
/// "bin/sh"` -> `"bash"`).
fn relative_target(dest: &Path, link: &Path) -> PathBuf {
    let dest_dir = dest.parent().unwrap_or_else(|| Path::new(""));
    let link_dir = link.parent().unwrap_or_else(|| Path::new(""));

    if dest_dir == link_dir {
        return PathBuf::from(dest.file_name().expect("dest has a file name"));
    }

    let mut target = PathBuf::new();
    for _ in link_dir.components() {
        target.push("..");
    }
    target.push(dest);
    target
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_directory_uses_bare_file_name() {
        assert_eq!(
            relative_target(Path::new("bin/bash"), Path::new("bin/sh")),
            PathBuf::from("bash")
        );
    }

    #[test]
    fn different_directory_walks_up() {
        assert_eq!(
            relative_target(Path::new("bin/busybox"), Path::new("sbin/init")),
            PathBuf::from("../bin/busybox")
        );
    }
}
