//! Dependency-order resolution: a real topological sort (Kahn's
//! algorithm) over each buildpack's declared `dependencies()`, replacing
//! today's hand-maintained sequential call order.

use crate::{BuildCtx, Buildpack};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::process::Command;

/// A named grouping of buildpack ids, purely organizational — no bearing
/// on build order, pruning, or anything else. Exists so the dependency
/// graph can draw a labeled box around packages that only really make
/// sense together (e.g. `cosmic_comp` + `cosmic_bg`: neither is a usable
/// "product" without the other, unlike a shared library such as
/// `libdrm`, which has no bundle of its own).
pub struct Bundle {
    pub name: &'static str,
    pub members: &'static [&'static str],
}

/// Ids of "final" buildpacks: those nothing else's `dependencies()` *or*
/// `functional_dependencies()` lists, i.e. no arrow of either kind leaves
/// them. Graph-theoretically these are the sinks — a real compositor, a
/// shell, an end-user binary — as opposed to shared infrastructure
/// something else consumes, whether at build time (`libdrm`) or only at
/// runtime (`xkeyboard_config`, once something declares a functional
/// dependency on it — before that it has no consumers of any kind yet,
/// so it's final too; "final" tracks the graph as declared, not some
/// permanent property of the package). Computed from the graph itself,
/// never hand-tagged, so it can't drift from reality as buildpacks are
/// added or their dependency lists change.
pub fn final_packages(packs: &[Box<dyn Buildpack>]) -> Vec<&'static str> {
    let index_of: HashMap<&'static str, usize> =
        packs.iter().enumerate().map(|(i, p)| (p.id(), i)).collect();
    let mut has_dependents = vec![false; packs.len()];
    for pack in packs {
        for dep_id in pack.dependencies().iter().chain(pack.functional_dependencies()) {
            if let Some(&dep_idx) = index_of.get(dep_id) {
                has_dependents[dep_idx] = true;
            }
        }
    }
    packs.iter().enumerate().filter(|&(i, _)| !has_dependents[i]).map(|(_, p)| p.id()).collect()
}

/// Whether putting `id` in `[build] disabled` would actually change
/// anything. True if `id` is itself final (the direct case), *or* if
/// some final package's own `functional_dependencies()` chain runs
/// through `id` (the cascading case — e.g. `cosmic_comp` stopped being
/// final the moment `cosmic_bg` gained a functional dependency on it,
/// but disabling `cosmic_comp` must still work, since that's exactly
/// what makes `cosmic_bg` functionally dead and drops it too).
/// `vulkan_headers` fails both checks — mesa's real dependency on it is a
/// *build-order* edge, not functional, so no final package's functional
/// chain ever reaches it, and disabling it directly would silently do
/// nothing (`prune_disabled` would still pull it back in through mesa).
pub fn is_disableable(packs: &[Box<dyn Buildpack>], id: &str) -> bool {
    let index_of: HashMap<&'static str, usize> =
        packs.iter().enumerate().map(|(i, p)| (p.id(), i)).collect();
    let Some(&target) = index_of.get(id) else { return false };

    for final_id in final_packages(packs) {
        let Some(&start) = index_of.get(final_id) else { continue };
        let mut seen = HashSet::new();
        let mut stack = vec![start];
        while let Some(i) = stack.pop() {
            if i == target {
                return true;
            }
            if !seen.insert(i) {
                continue;
            }
            for dep_id in packs[i].functional_dependencies() {
                if let Some(&dep_idx) = index_of.get(dep_id) {
                    stack.push(dep_idx);
                }
            }
        }
    }
    false
}

/// Ids to keep after removing `disabled` final packages, any other final
/// package that's functionally dead without one of them (e.g. `cosmic_bg`
/// is itself final — nothing builds against it — but useless without
/// `cosmic_comp`; disabling `cosmic_comp` drops `cosmic_bg` too, found by
/// walking `functional_dependencies()`), and anything whose only
/// remaining path forward led exclusively to one of those — computed as
/// backward reachability (both build-order *and* functional edges) from
/// every *surviving* final package (every non-final package is, by
/// construction, an ancestor of some final one via one edge type or the
/// other, so this can't accidentally orphan a real dependency still
/// needed elsewhere: it stays reachable from whichever other final
/// package still needs it).
///
/// A `required()` package must never appear in `functional_dependencies()`
/// on a *non*-required one — `functionally_dead` (below) would then treat
/// disabling that non-required package as a reason to drop the required
/// one too, silently defeating the whole point of `required()` (which
/// only guards direct entries in `disabled`, not this indirect path).
/// `check_required_invariant` catches that misconfiguration.
pub fn check_required_invariant(packs: &[Box<dyn Buildpack>]) -> Result<()> {
    let required: HashSet<&str> = packs.iter().filter(|p| p.required()).map(|p| p.id()).collect();
    for pack in packs {
        if !pack.required() {
            continue;
        }
        for dep_id in pack.functional_dependencies() {
            if !required.contains(dep_id) {
                bail!(
                    "\"{}\" is required() but has a functional dependency on \"{dep_id}\", \
                     which isn't — disabling \"{dep_id}\" would silently prune a required package",
                    pack.id()
                );
            }
        }
    }
    Ok(())
}

pub fn prune_disabled(packs: &[Box<dyn Buildpack>], disabled: &[String]) -> HashSet<&'static str> {
    let index_of: HashMap<&'static str, usize> =
        packs.iter().enumerate().map(|(i, p)| (p.id(), i)).collect();
    let disabled_idx: HashSet<usize> =
        disabled.iter().filter_map(|d| index_of.get(d.as_str()).copied()).collect();

    // True if `start` is disabled itself, or transitively functionally
    // depends on something disabled — so keeping it would just build a
    // client with nothing to connect to.
    let functionally_dead = |start: usize| -> bool {
        let mut seen = HashSet::new();
        let mut stack = vec![start];
        while let Some(i) = stack.pop() {
            if disabled_idx.contains(&i) {
                return true;
            }
            if !seen.insert(i) {
                continue;
            }
            for dep_id in packs[i].functional_dependencies() {
                if let Some(&dep_idx) = index_of.get(dep_id) {
                    stack.push(dep_idx);
                }
            }
        }
        false
    };

    let mut keep = HashSet::new();
    let mut stack: Vec<usize> = final_packages(packs)
        .into_iter()
        .filter_map(|id| index_of.get(id).copied())
        .filter(|&i| !functionally_dead(i))
        .collect();

    while let Some(i) = stack.pop() {
        if !keep.insert(i) {
            continue;
        }
        // Both edge kinds: a package only ever reachable via a functional
        // edge (xkeyboard_config, needed only at runtime) must be kept
        // exactly like a build-order one (libdrm) — final_packages now
        // excludes it from being final in its own right (something
        // depends on it), so without this it would never get pulled back
        // in and would vanish from every build the moment any pruning
        // runs at all.
        for dep_id in packs[i].dependencies().iter().chain(packs[i].functional_dependencies()) {
            if let Some(&dep_idx) = index_of.get(dep_id) {
                stack.push(dep_idx);
            }
        }
    }

    keep.into_iter().map(|i| packs[i].id()).collect()
}

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

/// Renders `packs`' dependency graph as Graphviz DOT source.
///
/// Solid edges are build-order `dependencies()` (prerequisite ->
/// dependent, the same direction `topo_order` builds its adjacency in);
/// dashed gray edges are `functional_dependencies()` — real but
/// runtime-only relationships (e.g. `cosmic_bg ⇢ cosmic_comp`) that never
/// influence build order. Final packages (`final_packages`: nothing
/// depends on them for building) get a bold border, marking them as the
/// products/leaves this graph exists to help choose between, rather than
/// shared infrastructure. `bundles` draws a labeled dashed box around
/// packages that only make sense together — purely organizational, no
/// effect on order or pruning.
///
/// `ctx_for` resolves each buildpack's `BuildCtx` (needed to call
/// `outputs()`, which some buildpacks — e.g. util-linux in `full` mode —
/// compute by scanning a build directory that may not exist yet; those
/// safely return an empty list rather than erroring). Any buildpack with
/// more than one declared output gets each one listed in its node label
/// (e.g. `shadow`'s `login`/`passwd`, `weston`'s compositor binary +
/// `.pc` marker) — with only one output, the id alone is enough.
pub fn to_dot(packs: &[Box<dyn Buildpack>], ctx_for: impl Fn(&str) -> BuildCtx, bundles: &[Bundle]) -> String {
    let finals: HashSet<&str> = final_packages(packs).into_iter().collect();
    let bundled: HashSet<&str> = bundles.iter().flat_map(|b| b.members.iter().copied()).collect();

    let node_decl = |pack: &Box<dyn Buildpack>| -> String {
        let outputs = pack.outputs(&ctx_for(pack.id()));
        let label = if outputs.len() > 1 {
            let mut lines = vec![pack.id().to_string()];
            lines.extend(outputs.iter().map(|o| format!("  {}", o.description)));
            lines.join("\\l") + "\\l"
        } else {
            pack.id().to_string()
        };
        let style = if finals.contains(pack.id()) { ", penwidth=2" } else { "" };
        format!("    \"{}\" [label=\"{label}\"{style}];\n", pack.id())
    };

    let mut dot =
        String::from("digraph buildpacks {\n    rankdir=LR;\n    node [shape=box, fontname=\"monospace\"];\n");

    for pack in packs {
        if !bundled.contains(pack.id()) {
            dot.push_str(&node_decl(pack));
        }
    }
    for (i, bundle) in bundles.iter().enumerate() {
        dot.push_str(&format!(
            "    subgraph cluster_{i} {{\n        label=\"{}\";\n        style=dashed;\n",
            bundle.name
        ));
        for pack in packs.iter().filter(|p| bundle.members.contains(&p.id())) {
            dot.push_str("    ");
            dot.push_str(&node_decl(pack));
        }
        dot.push_str("    }\n");
    }

    for pack in packs {
        for dep in pack.dependencies() {
            dot.push_str(&format!("    \"{}\" -> \"{}\";\n", dep, pack.id()));
        }
        for dep in pack.functional_dependencies() {
            dot.push_str(&format!(
                "    \"{}\" -> \"{}\" [style=dashed, color=gray40];\n",
                dep,
                pack.id()
            ));
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
pub fn write_svg(
    packs: &[Box<dyn Buildpack>],
    ctx_for: impl Fn(&str) -> BuildCtx,
    bundles: &[Bundle],
    path: &Path,
) -> Result<()> {
    let dot = to_dot(packs, ctx_for, bundles);
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
