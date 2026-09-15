# linux-builder

A Cargo workspace that builds two independent, bootable Linux systems
entirely from upstream source: `distro` (glibc, from-scratch, aimed at a
COSMIC desktop) and `distroless` (musl + BusyBox + uutils, minimal).
Shared architecture lives in `buildpack-core`/`buildpacks` (the
`Buildpack` trait every package implements) and `builder-tui`. See
`docs/distro-phase-report.typ` for the full technical report.

## Rules

- **Rust and C only for anything that ships on target.** Any software
  that ends up in the built image — a buildpack's runtime output,
  daemons, compositors, COSMIC components, future apps — must be
  implemented in Rust or C. No Python, Ruby, Perl, or Node runtimes ship
  on target. Host-side build tooling is exempt: `meson` (Python) is
  infrastructure, not distro content, the same carve-out as `gcc`/
  `ninja`/`pkg-config`. Only a package's *runtime* language matters, not
  its build system. Check this before picking an implementation for any
  new buildpack (network stack, audio stack, COSMIC components, etc.).
