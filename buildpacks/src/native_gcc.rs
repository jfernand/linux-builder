//! A minimal, link-only GCC + binutils on the target image — not to
//! compile C source (nothing here ships or needs a C compiler on
//! target), but because `rustc` itself still shells out to `cc` as a
//! linker *driver* by default on `x86_64-unknown-linux-gnu`, even when
//! the actual linking backend is `rust_toolchain`'s own bundled
//! `rust-lld`. Confirmed against rustc's own docs during planning:
//! full self-contained linking (no external `cc` at all) is an explicit,
//! unstable, no-timeline work in progress — not something to build on.
//! `rustc -o out file.o` / `cargo build` both failed outright with
//! `error: linker `cc` not found` before this buildpack existed.
//!
//! Link-only means the *compiler frontend* (`cc1`/`cc1plus`, ~30MB each)
//! is deliberately left out — `rustc` already does its own codegen and
//! only invokes `cc`/`ld` to assemble the final binary from object files
//! it already produced. What's actually needed: the `gcc` driver binary
//! itself (to parse `-o`/`-l...` flags and locate the right paths),
//! `collect2` (gcc's own linking helper), the real linker (`ld`),
//! gcc's own small internal objects/archives (`crtbegin*.o`/
//! `crtend*.o`/`libgcc*.a`), and glibc's CRT startup objects
//! (`crt1.o`/`crti.o`/`crtn.o`/`Scrt1.o` — these come from glibc, not
//! gcc, same "we don't build glibc from source" reasoning as
//! `HOST_DYNAMIC_LIBS`/`install_locale_data` in `rootfs.rs`).
//!
//! Copied straight from the host, same category as those two — this
//! project's own gcc/binutils, whatever version the host actually has
//! (concretely GCC 13 on this host, hardcoded the same way
//! `install_locale_data`'s exact host paths are, not dynamically
//! discovered), not something to build or version-pin independently.

use anyhow::{Context, Result};
use buildpack_core::run::{already_built, run_in};
use buildpack_core::{BuildCtx, BuildOutput, Buildpack, Description, InstallMode, Source};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

const GCC_TRIPLE_BIN: &str = "/usr/bin/x86_64-linux-gnu-gcc-13";
const LD_TRIPLE_BIN: &str = "/usr/bin/x86_64-linux-gnu-ld.bfd";
const COLLECT2: &str = "/usr/libexec/gcc/x86_64-linux-gnu/13/collect2";
// This host's Ubuntu-packaged gcc-13 defaults to `-fuse-linker-plugin`
// even for a plain link-only invocation (no `-flto` requested) — found
// the hard way via a real boot test: `cc: fatal error:
// '-fuse-linker-plugin', but liblto_plugin.so not found`. Small (~70KB),
// no real reason to fight the default instead of just shipping it.
const LTO_PLUGIN: &str = "/usr/libexec/gcc/x86_64-linux-gnu/13/liblto_plugin.so";
const GCC_LIB_DIR: &str = "/usr/lib/gcc/x86_64-linux-gnu/13";
const GLIBC_CRT_OBJECTS: &[&str] = &["crt1.o", "crti.o", "crtn.o", "Scrt1.o"];
const GLIBC_LIB_DIR: &str = "/usr/lib/x86_64-linux-gnu";

// The *development* linking stubs `-lc`/`-lm`/`-ldl`/`-lpthread`/`-lrt`/
// `-lutil` resolve against — separate from the versioned runtime `.so.N`
// files `HOST_DYNAMIC_LIBS` already copies for actually running things.
// Found the hard way: rust-lld (rustc's default linker, invoked via
// `-fuse-ld=lld`/`-B.../bin/gcc-ld`, bypassing the real `ld.bfd` this
// buildpack also ships entirely) failed with "unable to find library
// -lc"/"-lm"/etc — none of these had ever been copied in. `libc.so`/
// `libm.so` are tiny GNU-ld GROUP scripts (plain text, referencing other
// absolute paths this rootfs already has at the same locations); glibc
// 2.34+ folded `libdl`/`libpthread`/`librt`/`libutil`'s real
// implementations into `libc.so.6` itself and only ships empty 8-byte
// placeholder `.a` archives for them now (no `.so` variants exist on
// this host at all) — still real files a linker's `-lname` search needs
// to find, even though they're functionally inert.
const GLIBC_DEV_LINK_FILES: &[&str] = &[
    "libc.so",
    "libc_nonshared.a",
    "libm.so",
    "libdl.a",
    "libpthread.a",
    "libpthread_nonshared.a",
    "librt.a",
    "libutil.a",
];

// ld itself needs a few more host libraries beyond what's already in
// rootfs.rs's HOST_DYNAMIC_LIBS — binutils' own dependencies, not
// something anything else here has pulled in yet.
const LD_HOST_LIBS: &[&str] = &["libbfd-2.42-system.so", "libctf.so.0", "libjansson.so.4", "libsframe.so.1"];

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct NativeGccConfig {}

#[derive(Default)]
pub struct NativeGcc {
    cfg: NativeGccConfig,
}

impl NativeGcc {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Buildpack for NativeGcc {
    fn id(&self) -> &'static str {
        "native_gcc"
    }

    fn configure(&mut self, table: &toml::Value) -> Result<()> {
        self.cfg = table.clone().try_into().context("parsing [native_gcc] config")?;
        Ok(())
    }

    fn to_toml(&self) -> Result<toml::Value> {
        toml::Value::try_from(&self.cfg).context("serializing [native_gcc] config")
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    fn describe(&self) -> Description {
        Description {
            id: "native_gcc",
            name: "native gcc (link-only)",
            summary: "cc/ld on the target image, so rustc can actually link a binary",
            long_description: "Copied straight from the host, same category as \
                HOST_DYNAMIC_LIBS/install_locale_data in rootfs.rs. No C compiler frontend \
                (cc1/cc1plus) — link-only: the gcc driver, collect2, ld, gcc's own crtbegin/ \
                crtend/libgcc archives, and glibc's CRT startup objects.",
        }
    }

    fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
        vec![Source::InTree]
    }

    fn build(&self, ctx: &BuildCtx, force: bool) -> Result<()> {
        let marker = ctx.sysroot_dir.join("usr/bin/cc");
        if already_built(&marker, force) {
            println!("skip build-native-gcc: {} already exists", marker.display());
            return Ok(());
        }

        println!("copying native gcc/binutils (link-only) into sysroot");

        let bin_dir = ctx.sysroot_dir.join("usr/bin");
        fs::create_dir_all(&bin_dir).context("creating sysroot usr/bin")?;
        copy_file(Path::new(GCC_TRIPLE_BIN), &bin_dir.join("x86_64-linux-gnu-gcc-13"))?;
        copy_file(Path::new(LD_TRIPLE_BIN), &bin_dir.join("x86_64-linux-gnu-ld.bfd"))?;
        symlink(&bin_dir, "x86_64-linux-gnu-gcc-13", "gcc")?;
        symlink(&bin_dir, "x86_64-linux-gnu-ld.bfd", "x86_64-linux-gnu-ld")?;
        symlink(&bin_dir, "x86_64-linux-gnu-ld", "ld")?;

        // `cc` is a thin wrapper, not a plain symlink to `gcc`: this
        // Ubuntu-packaged gcc-13's own specs (`gcc -dumpspecs`) always
        // engage its linker-plugin path — `-plugin %(linker_plugin_file)
        // -plugin-opt=%(lto_wrapper) ...` — unless `-fno-use-linker-plugin`
        // or `-fno-lto` is passed, regardless of whether any real LTO
        // bitcode is involved. Found the hard way: rustc's default
        // `-fuse-ld=lld` (its own bundled rust-lld, not the real `ld.bfd`
        // this buildpack also ships) makes collect2 think the linker
        // supports the plugin protocol and engages it anyway, but
        // `%(lto_wrapper)` expands empty without the `lto-wrapper`
        // program this buildpack deliberately doesn't ship (compiler-
        // frontend-adjacent, not needed for pure linking) — `rust-lld:
        // error: -plugin-opt=: unknown plugin option ''`. Not something
        // rustc's own invocation can be changed to avoid (it doesn't
        // know or care about this host's specific GCC packaging
        // default), so `cc` itself defends against it instead.
        // `fs::write`/`fs::set_permissions` both follow symlinks — with
        // no prior removal, writing to a `cc` that's *already* a symlink
        // (e.g. from a previous build of this buildpack) would silently
        // write through the whole `cc` -> `gcc` -> `x86_64-linux-gnu-gcc-13`
        // chain and corrupt the real compiler binary in place. Found by
        // actually hitting it: a `--force` rebuild overwrote
        // `x86_64-linux-gnu-gcc-13` with this wrapper script's own text.
        let cc_wrapper_path = bin_dir.join("cc");
        if cc_wrapper_path.exists() || cc_wrapper_path.is_symlink() {
            fs::remove_file(&cc_wrapper_path).ok();
        }
        fs::write(&cc_wrapper_path, "#!/bin/sh\nexec /usr/bin/gcc -fno-use-linker-plugin \"$@\"\n")
            .context("writing cc wrapper script")?;
        fs::set_permissions(&cc_wrapper_path, fs::Permissions::from_mode(0o755))
            .context("making cc wrapper executable")?;

        let libexec_dir = ctx.sysroot_dir.join("usr/libexec/gcc/x86_64-linux-gnu/13");
        fs::create_dir_all(&libexec_dir).context("creating sysroot gcc libexec dir")?;
        copy_file(Path::new(COLLECT2), &libexec_dir.join("collect2"))?;
        copy_file(Path::new(LTO_PLUGIN), &libexec_dir.join("liblto_plugin.so"))?;

        let gcc_lib_parent = ctx.sysroot_dir.join("usr/lib/gcc/x86_64-linux-gnu");
        fs::create_dir_all(&gcc_lib_parent).context("creating sysroot gcc lib parent dir")?;
        run_in(
            Path::new("."),
            Command::new("cp").arg("-a").arg(GCC_LIB_DIR).arg(&gcc_lib_parent),
        )?;

        let glibc_lib_dest = ctx.sysroot_dir.join("usr/lib/x86_64-linux-gnu");
        fs::create_dir_all(&glibc_lib_dest).context("creating sysroot glibc lib dir")?;
        for name in GLIBC_CRT_OBJECTS {
            copy_file(&Path::new(GLIBC_LIB_DIR).join(name), &glibc_lib_dest.join(name))?;
        }
        for name in LD_HOST_LIBS {
            copy_file(&Path::new(GLIBC_LIB_DIR).join(name), &glibc_lib_dest.join(name))?;
        }
        for name in GLIBC_DEV_LINK_FILES {
            copy_file(&Path::new(GLIBC_LIB_DIR).join(name), &glibc_lib_dest.join(name))?;
        }

        Ok(())
    }

    fn outputs(&self, ctx: &BuildCtx) -> Vec<BuildOutput> {
        vec![BuildOutput {
            description: "cc (sysroot marker)".to_string(),
            path: ctx.sysroot_dir.join("usr/bin/cc"),
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

fn copy_file(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest.parent().unwrap())
        .with_context(|| format!("creating parent dir for {}", dest.display()))?;
    fs::copy(src, dest).with_context(|| format!("copying {} to {}", src.display(), dest.display()))?;
    Ok(())
}

fn symlink(dir: &Path, target: &str, link_name: &str) -> Result<()> {
    let link_path = dir.join(link_name);
    if link_path.exists() || link_path.is_symlink() {
        fs::remove_file(&link_path).ok();
    }
    std::os::unix::fs::symlink(target, &link_path)
        .with_context(|| format!("symlinking {} -> {}", link_path.display(), target))
}
