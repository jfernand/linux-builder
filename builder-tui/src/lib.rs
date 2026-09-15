//! A generic TUI dashboard for a `Buildpack`/`PipelineStage`-driven distro
//! pipeline — the re-exec/event-loop/settings machinery is identical
//! between `distro` and `distroless` (both share `DistroConfig` and
//! `buildpacks::kernel`), so it lives here once; each distro's own
//! package set is the one real difference, injected via `Registry`.

mod app;
mod dashboard;
mod registry;
mod stage;

pub use dashboard::run;
pub use registry::Registry;
pub use stage::StageKind;
