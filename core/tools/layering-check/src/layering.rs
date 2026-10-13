//! The layering rules of edges.toml applied to a workspace's `cargo metadata`.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use crate::edges::{EdgeKind, Edges};
use crate::metadata::{Metadata, Node};
use crate::{Error, Rule, Violation, raw_open};

/// Every violation of `edges` in the workspace, sorted by rule and subject.
pub fn check(meta: &Metadata, edges: &Edges) -> Result<Vec<Violation>, Error> {
    let graph = Graph::new(meta)?;
    let mut violations = Vec::new();
    placement(&graph, edges, &mut violations);
    member_edges(&graph, edges, &mut violations);
    restricted(&graph, edges, &mut violations);
    violations.extend(raw_open::scan(meta, edges)?);
    violations.sort();
    violations.dedup();
    Ok(violations)
}

struct Graph<'a> {
    /// Workspace members: package id -> name.
    members: BTreeMap<&'a str, &'a str>,
    names: HashMap<&'a str, &'a str>,
    nodes: HashMap<&'a str, &'a Node>,
}

impl<'a> Graph<'a> {
    fn new(meta: &'a Metadata) -> Result<Graph<'a>, Error> {
        let resolve = meta
            .resolve
            .as_ref()
            .ok_or_else(|| Error::new("cargo metadata has no resolve graph"))?;
        let names: HashMap<&str, &str> = meta.packages.iter().map(|p| (p.id.as_str(), p.name.as_str())).collect();
        let mut members = BTreeMap::new();
        for id in &meta.workspace_members {
            let name = names
                .get(id.as_str())
                .ok_or_else(|| Error::new(format!("unknown workspace member {id}")))?;
            members.insert(id.as_str(), *name);
        }
        let nodes = resolve.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
        Ok(Graph { members, names, nodes })
    }

    fn name(&self, id: &str) -> &'a str {
        self.names.get(id).copied().unwrap_or("?")
    }
}

fn placement(graph: &Graph, edges: &Edges, out: &mut Vec<Violation>) {
    let member_names: BTreeSet<&str> = graph.members.values().copied().collect();
    for name in &member_names {
        if edges.place(name).is_none() {
            out.push(Violation::new(
                Rule::UnknownCrate,
                *name,
                format!("workspace crate {name} is not in edges.toml; add it to a row, [any] or [isolated]"),
            ));
        }
    }
    for name in edges.names() {
        if !member_names.contains(name) {
            out.push(Violation::new(
                Rule::StaleEntry,
                name,
                format!("edges.toml names {name}, which is not a workspace crate"),
            ));
        }
    }
}

fn member_edges(graph: &Graph, edges: &Edges, out: &mut Vec<Violation>) {
    for (&id, &from) in &graph.members {
        let Some(node) = graph.nodes.get(id) else { continue };
        for dep in &node.deps {
            let Some(&to) = graph.members.get(dep.pkg.as_str()) else {
                continue;
            };
            let kinds: BTreeSet<Option<&str>> = dep.dep_kinds.iter().map(|k| k.kind.as_deref()).collect();
            let kind = if kinds.contains(&None) {
                EdgeKind::Normal
            } else if kinds.contains(&Some("build")) {
                EdgeKind::Build
            } else {
                EdgeKind::Dev
            };
            if let Some(message) = edges.deny(from, to, kind) {
                out.push(Violation::new(Rule::Edge, format!("{from} -> {to}"), message));
            }
        }
    }
}

/// A restricted crate outside its allowed crates, as a direct dependency or
/// reached through third-party crates (never through another member: that
/// member is checked on its own).
fn restricted(graph: &Graph, edges: &Edges, out: &mut Vec<Violation>) {
    for (&id, &member) in &graph.members {
        for (target, rule) in &edges.restricted {
            if rule.allowed.iter().any(|a| a == member) {
                continue;
            }
            if let Some(path) = path_to(graph, id, target) {
                let allowed = rule.allowed.join(", ");
                let message = if path.len() == 1 {
                    format!("{member} depends on {target}; edges.toml allows {target} only in {allowed}")
                } else {
                    format!(
                        "{member} reaches {target} through {}; edges.toml allows {target} only in {allowed}",
                        path.join(" -> ")
                    )
                };
                out.push(Violation::new(
                    Rule::Restricted,
                    format!("{member} -> {target}"),
                    message,
                ));
            }
        }
    }
}

/// The shortest chain of crate names from `start` to a crate named `target`.
fn path_to<'a>(graph: &Graph<'a>, start: &'a str, target: &str) -> Option<Vec<&'a str>> {
    let mut seen = HashSet::from([start]);
    let mut queue = VecDeque::from([(start, Vec::new())]);
    while let Some((id, path)) = queue.pop_front() {
        let Some(&node) = graph.nodes.get(id) else { continue };
        for dep in &node.deps {
            let dep_id = dep.pkg.as_str();
            if !seen.insert(dep_id) {
                continue;
            }
            let mut next = path.clone();
            next.push(graph.name(dep_id));
            if graph.name(dep_id) == target {
                return Some(next);
            }
            if !graph.members.contains_key(dep_id) {
                queue.push_back((dep_id, next));
            }
        }
    }
    None
}
