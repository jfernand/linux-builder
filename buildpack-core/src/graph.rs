//! Dependency-order resolution: a real topological sort (Kahn's
//! algorithm) over each buildpack's declared `dependencies()`, replacing
//! today's hand-maintained sequential call order.

use crate::{BuildCtx, Buildpack};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::process::Command;

/// Returns indices into `packs`, in an order that respects every declared
/// dependency edge (a dependency's index always precedes its dependents').
/// Errors if the dependency graph has a cycle.
///
/// A declared dependency id that isn't present in `packs` is silently
/// treated as external/already-satisfied, not an error — `packs` is often
/// a deliberate *subset* of the full package set (e.g. Mesa's real
/// dependencies include libdrm/wayland/libxkbcommon/pixman/
/// libdisplay_info/libinput, none of which are buildpacks yet; running
/// just the buildpack-based subset of a larger pipeline is a normal case,
/// not a registration bug).
pub fn topo_order(packs: &[Box<dyn Buildpack>]) -> Result<Vec<usize>> {
    let index_of: HashMap<&'static str, usize> =
        packs.iter().enumerate().map(|(i, p)| (p.id(), i)).collect();

    // adjacency: dependency -> dependents (edges point from prerequisite to
    // the thing that needs it, matching build order)
    let mut adjacency: Vec<Vec<usize>> = vec![Vec::new(); packs.len()];
    let mut in_degree: Vec<usize> = vec![0; packs.len()];

    for (i, pack) in packs.iter().enumerate() {
        for dep_id in pack.dependencies() {
            let Some(&dep_idx) = index_of.get(dep_id) else { continue };
            adjacency[dep_idx].push(i);
            in_degree[i] += 1;
        }
    }

    let mut queue: VecDeque<usize> =
        (0..packs.len()).filter(|&i| in_degree[i] == 0).collect();
    let mut order = Vec::with_capacity(packs.len());

    while let Some(i) = queue.pop_front() {
        order.push(i);
        for &next in &adjacency[i] {
            in_degree[next] -= 1;
            if in_degree[next] == 0 {
                queue.push_back(next);
            }
        }
    }

    if order.len() != packs.len() {
        let stuck: Vec<&str> = (0..packs.len())
            .filter(|&i| in_degree[i] > 0)
            .map(|i| packs[i].id())
            .collect();
        bail!("dependency cycle detected among: {}", stuck.join(", "));
    }

    Ok(order)
}

/// Renders `packs`' dependency graph as Graphviz DOT source — an edge per
/// declared `dependencies()` entry, pointing from prerequisite to
/// dependent (the same direction `topo_order` builds its adjacency in).
/// `ctx_for` resolves each buildpack's `BuildCtx` (needed to call
/// `outputs()`, which some buildpacks — e.g. util-linux in `full` mode —
/// compute by scanning a build directory that may not exist yet; those
/// safely return an empty list rather than erroring). Any buildpack with
/// more than one declared output gets each one listed in its node label
/// (e.g. `shadow`'s `login`/`passwd`, `weston`'s compositor binary +
/// `.pc` marker) — with only one output, the id alone is enough.
pub fn to_dot(packs: &[Box<dyn Buildpack>], ctx_for: impl Fn(&str) -> BuildCtx) -> String {
    let mut dot =
        String::from("digraph buildpacks {\n    rankdir=LR;\n    node [shape=box, fontname=\"monospace\"];\n");
    for pack in packs {
        let outputs = pack.outputs(&ctx_for(pack.id()));
        let label = if outputs.len() > 1 {
            let mut lines = vec![pack.id().to_string()];
            lines.extend(outputs.iter().map(|o| format!("  {}", o.description)));
            lines.join("\\l") + "\\l"
        } else {
            pack.id().to_string()
        };
        dot.push_str(&format!("    \"{}\" [label=\"{label}\"];\n", pack.id()));
        for dep in pack.dependencies() {
            dot.push_str(&format!("    \"{}\" -> \"{}\";\n", dep, pack.id()));
        }
    }
    dot.push_str("}\n");
    dot
}

/// Writes `packs`' dependency graph as an SVG to `path`, via the `dot`
/// command (graphviz). Meant to be called on every real build run, not
/// just on request — a standing, always-current picture of what depends
/// on what and what each package produces, since `dependencies()`/
/// `outputs()` are the only places that information is declared today (no
/// separate diagram to keep in sync by hand).
pub fn write_svg(packs: &[Box<dyn Buildpack>], ctx_for: impl Fn(&str) -> BuildCtx, path: &Path) -> Result<()> {
    let dot = to_dot(packs, ctx_for);
    let output = Command::new("dot")
        .args(["-Tsvg"])
        .arg("-o")
        .arg(path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child.stdin.take().unwrap().write_all(dot.as_bytes())?;
            child.wait_with_output()
        })
        .with_context(|| format!("running dot -Tsvg -o {}", path.display()))?;
    if !output.status.success() {
        bail!("dot failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BuildCtx, BuildOutput, Description, InstallMode, Source};
    use anyhow::Result;
    use std::any::Any;

    struct Stub {
        id: &'static str,
        deps: &'static [&'static str],
    }

    impl Buildpack for Stub {
        fn id(&self) -> &'static str {
            self.id
        }
        fn configure(&mut self, _table: &toml::Value) -> Result<()> {
            Ok(())
        }
        fn to_toml(&self) -> Result<toml::Value> {
            Ok(toml::Value::Table(Default::default()))
        }
        fn dependencies(&self) -> &'static [&'static str] {
            self.deps
        }
        fn describe(&self) -> Description {
            Description { id: self.id, name: self.id, summary: "", long_description: "" }
        }
        fn sources(&self, _ctx: &BuildCtx) -> Vec<Source> {
            Vec::new()
        }
        fn build(&self, _ctx: &BuildCtx, _force: bool) -> Result<()> {
            Ok(())
        }
        fn outputs(&self, _ctx: &BuildCtx) -> Vec<BuildOutput> {
            Vec::new()
        }
        fn install_mode(&self) -> InstallMode {
            InstallMode::StaticArtifacts
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    fn stub(id: &'static str, deps: &'static [&'static str]) -> Box<dyn Buildpack> {
        Box::new(Stub { id, deps })
    }

    fn assert_before(order: &[usize], packs: &[Box<dyn Buildpack>], first: &str, second: &str) {
        let pos = |id: &str| order.iter().position(|&i| packs[i].id() == id).unwrap();
        assert!(pos(first) < pos(second), "{first} should come before {second}");
    }

    #[test]
    fn linear_chain() {
        let packs = vec![stub("a", &[]), stub("b", &["a"]), stub("c", &["b"])];
        let order = topo_order(&packs).unwrap();
        assert_eq!(order.len(), 3);
        assert_before(&order, &packs, "a", "b");
        assert_before(&order, &packs, "b", "c");
    }

    #[test]
    fn diamond() {
        // mesa depends on libdrm and wayland; both depend on nothing here,
        // shaped like the real libdrm/wayland/mesa relationship.
        let packs = vec![
            stub("libdrm", &[]),
            stub("wayland", &[]),
            stub("mesa", &["libdrm", "wayland"]),
        ];
        let order = topo_order(&packs).unwrap();
        assert_eq!(order.len(), 3);
        assert_before(&order, &packs, "libdrm", "mesa");
        assert_before(&order, &packs, "wayland", "mesa");
    }

    #[test]
    fn cycle_detected() {
        let packs = vec![stub("a", &["b"]), stub("b", &["a"])];
        let err = topo_order(&packs).unwrap_err();
        assert!(err.to_string().contains("cycle"));
    }

    #[test]
    fn unknown_dependency_is_treated_as_external() {
        // "b" depends on "nonexistent", which isn't in this registry —
        // treated as already-satisfied, not an error (see Mesa's real
        // dependencies() for why this is the normal case, not a bug).
        let packs = vec![stub("a", &[]), stub("b", &["nonexistent"])];
        let order = topo_order(&packs).unwrap();
        assert_eq!(order.len(), 2);
    }
}
