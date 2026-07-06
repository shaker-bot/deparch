//! Language-agnostic analysis over a normalized `Analysis`.

use crate::model::*;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Serialize)]
pub struct Finding {
    pub language: String,
    pub unused: Vec<UnusedDep>,
    /// Declared + not imported, but a hint says it's used another way.
    pub suppressed: Vec<SuppressedDep>,
    pub phantom: Vec<PhantomDep>,
}

#[derive(Serialize)]
pub struct UnusedDep {
    pub name: String,
    pub kind: DepKind,
    pub manifest: String,
}

#[derive(Serialize)]
pub struct SuppressedDep {
    pub name: String,
    pub kind: DepKind,
    pub manifest: String,
    pub reason: String,
}

#[derive(Serialize)]
pub struct PhantomDep {
    pub name: String,
    pub version: String,
    pub example_file: String,
    pub example_line: usize,
}

/// Cross-reference declared vs. installed vs. imported.
pub fn analyze(a: &Analysis) -> Finding {
    let used_set: HashSet<&str> = a.used.iter().map(|u| u.package.as_str()).collect();
    let declared_set: HashSet<&str> = a.declared.iter().map(|d| d.name.as_str()).collect();

    // declared but nothing imports it -> either confidently unused, or
    // suppressed because a hint explains a non-import use.
    let mut unused = Vec::new();
    let mut suppressed = Vec::new();
    for d in &a.declared {
        if used_set.contains(d.name.as_str()) {
            continue;
        }
        if let Some(reason) = a.usage_hints.get(&d.name) {
            suppressed.push(SuppressedDep {
                name: d.raw_name.clone(),
                kind: d.kind,
                manifest: d.manifest.clone(),
                reason: reason.clone(),
            });
        } else {
            unused.push(UnusedDep {
                name: d.raw_name.clone(),
                kind: d.kind,
                manifest: d.manifest.clone(),
            });
        }
    }

    // phantom: imported + installed, but never declared (relying on a transitive).
    let mut seen = HashSet::new();
    let mut phantom = Vec::new();
    for u in &a.used {
        if declared_set.contains(u.package.as_str()) {
            continue;
        }
        if !seen.insert(u.package.clone()) {
            continue;
        }
        if let Some(pkg) = a.installed.get(&u.package) {
            phantom.push(PhantomDep {
                name: pkg.name.clone(),
                version: pkg.version.clone(),
                example_file: u.import.file.clone(),
                example_line: u.import.line,
            });
        }
    }

    Finding {
        language: a.language.clone(),
        unused,
        suppressed,
        phantom,
    }
}

/// "Why is this here": reverse-walk the resolved tree from `target` up to the
/// direct/declared dependency (or roots) that pulled it in.
pub fn why(a: &Analysis, target_raw: &str) -> Vec<Vec<String>> {
    let target = normalize(target_raw);
    if !a.installed.contains_key(&target) {
        return vec![];
    }

    // child (normalized) -> parents that require it (display names)
    let mut parents: HashMap<&str, Vec<&str>> = HashMap::new();
    for pkg in a.installed.values() {
        for req in &pkg.requires {
            parents.entry(req.as_str()).or_default().push(pkg.name.as_str());
        }
    }

    let declared: HashSet<&str> = a.declared.iter().map(|d| d.name.as_str()).collect();
    let mut chains = Vec::new();
    let mut path = vec![target.clone()];
    dfs(&target, &parents, &declared, &mut path, &mut chains, 0);
    chains
}

fn dfs(
    node: &str,
    parents: &HashMap<&str, Vec<&str>>,
    declared: &HashSet<&str>,
    path: &mut Vec<String>,
    out: &mut Vec<Vec<String>>,
    depth: usize,
) {
    let ps = parents.get(node);
    let is_root = declared.contains(node) || ps.map_or(true, |v| v.is_empty());
    if is_root {
        let mut c = path.clone();
        c.reverse();
        out.push(c);
        if declared.contains(node) {
            return; // stop at a declared direct dep
        }
    }
    if depth > 50 {
        return;
    }
    if let Some(ps) = ps {
        for p in ps {
            let pn = normalize(p);
            if path.iter().any(|x| normalize(x) == pn) {
                continue; // cycle guard
            }
            path.push(p.to_string());
            dfs(&pn, parents, declared, path, out, depth + 1);
            path.pop();
        }
    }
}
