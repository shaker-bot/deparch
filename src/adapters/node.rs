//! Node / TypeScript adapter.
//! declared: package.json  ·  installed: package-lock.json  ·  imports: parse_ts.js

use super::Adapter;
use crate::model::*;
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub struct NodeAdapter;

const SCRIPT: &str = include_str!("../scripts/parse_ts.js");

#[derive(serde::Deserialize)]
struct RawImport {
    specifier: String,
    file: String,
    line: usize,
}

impl Adapter for NodeAdapter {
    fn language(&self) -> &'static str {
        "node"
    }

    fn detect(&self, root: &Path) -> bool {
        root.join("package.json").exists()
    }

    fn analyze(&self, root: &Path) -> Result<Analysis> {
        let raw = super::run_script(SCRIPT, "cjs", "node", root).context("running node parser")?;
        let imports: Vec<RawImport> =
            serde_json::from_str(&raw).context("parsing node parser output")?;

        let declared = read_declared(root)?;
        let installed = read_lockfile(root)?;

        let mut used = Vec::new();
        for imp in imports {
            if let Some(pkg) = spec_to_package(&imp.specifier) {
                used.push(Usage {
                    package: pkg,
                    import: SourceImport {
                        specifier: imp.specifier,
                        file: imp.file,
                        line: imp.line,
                    },
                });
            }
        }

        let used_set: HashSet<String> = used.iter().map(|u| u.package.clone()).collect();
        let usage_hints = compute_hints(root, &declared, &used_set);

        Ok(Analysis {
            language: "node".into(),
            declared,
            installed,
            used,
            usage_hints,
        })
    }
}

fn read_declared(root: &Path) -> Result<Vec<DeclaredDep>> {
    let txt = std::fs::read_to_string(root.join("package.json"))?;
    let v: serde_json::Value = serde_json::from_str(&txt)?;
    let mut deps = Vec::new();
    for (field, kind) in [
        ("dependencies", DepKind::Prod),
        ("devDependencies", DepKind::Dev),
        ("optionalDependencies", DepKind::Optional),
        ("peerDependencies", DepKind::Peer),
    ] {
        if let Some(obj) = v.get(field).and_then(|x| x.as_object()) {
            for name in obj.keys() {
                deps.push(DeclaredDep {
                    name: normalize(name),
                    raw_name: name.clone(),
                    kind,
                    manifest: "package.json".into(),
                });
            }
        }
    }
    Ok(deps)
}

fn read_lockfile(root: &Path) -> Result<HashMap<String, ResolvedPkg>> {
    let mut map = HashMap::new();
    let lock = root.join("package-lock.json");
    if !lock.exists() {
        return Ok(map);
    }
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(lock)?)?;

    // lockfile v2/v3: `packages` maps "node_modules/<name>" -> info
    if let Some(pkgs) = v.get("packages").and_then(|p| p.as_object()) {
        for (path, info) in pkgs {
            if path.is_empty() {
                continue; // the root project itself
            }
            let name = path
                .rsplit_once("node_modules/")
                .map(|(_, n)| n)
                .unwrap_or(path)
                .to_string();
            map.insert(normalize(&name), resolved(&name, info));
        }
    } else if let Some(deps) = v.get("dependencies").and_then(|p| p.as_object()) {
        // lockfile v1
        for (name, info) in deps {
            map.insert(normalize(name), resolved(name, info));
        }
    }
    Ok(map)
}

fn resolved(name: &str, info: &serde_json::Value) -> ResolvedPkg {
    let version = info
        .get("version")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let mut requires = Vec::new();
    for f in ["dependencies", "optionalDependencies", "peerDependencies", "requires"] {
        if let Some(obj) = info.get(f).and_then(|x| x.as_object()) {
            for dn in obj.keys() {
                requires.push(normalize(dn));
            }
        }
    }
    ResolvedPkg {
        name: name.to_string(),
        version,
        requires,
    }
}

/// Config/build files (repo root) that can reference a package by string
/// rather than importing it.
const CONFIG_FILES: &[&str] = &[
    ".eslintrc", ".eslintrc.js", ".eslintrc.cjs", ".eslintrc.json", ".eslintrc.yml",
    ".eslintrc.yaml", "eslint.config.js", "eslint.config.mjs", "eslint.config.cjs",
    ".prettierrc", ".prettierrc.json", ".prettierrc.js", ".prettierrc.cjs", ".prettierrc.yml",
    ".prettierrc.yaml", "prettier.config.js", "prettier.config.cjs",
    "babel.config.js", "babel.config.cjs", "babel.config.json", ".babelrc", ".babelrc.js",
    ".babelrc.json", "postcss.config.js", "postcss.config.cjs", "tailwind.config.js",
    "tailwind.config.cjs", "tailwind.config.ts", "jest.config.js", "jest.config.cjs",
    "jest.config.ts", "jest.config.json", "jest.config.mjs", "vitest.config.js",
    "vitest.config.ts", "vite.config.js", "vite.config.ts", "vite.config.mjs",
    ".stylelintrc", ".stylelintrc.json", ".stylelintrc.js", "stylelint.config.js",
    "rollup.config.js", "rollup.config.mjs", "webpack.config.js", "webpack.config.cjs",
    "commitlint.config.js", ".commitlintrc.json", ".lintstagedrc", ".lintstagedrc.json",
    ".lintstagedrc.js", "tsconfig.json", "tsconfig.base.json", ".mocharc.js", ".mocharc.json",
    ".mocharc.cjs", "nodemon.json", "svelte.config.js", "astro.config.mjs", "next.config.js",
    "next.config.mjs", "nuxt.config.js", "nuxt.config.ts", "playwright.config.ts",
];

/// Determine whether a declared dep is likely used despite no source import.
fn compute_hints(
    root: &Path,
    declared: &[DeclaredDep],
    used: &HashSet<String>,
) -> HashMap<String, String> {
    let mut hints = HashMap::new();
    let config_blob = config_blob(root);
    for d in declared {
        // Type stubs: @types/x is used whenever x is used (or ambient globals).
        if let Some(base) = d.raw_name.strip_prefix("@types/") {
            const AMBIENT: &[&str] = &["node", "bun", "deno", "jest", "mocha"];
            if AMBIENT.contains(&base) || used.contains(&normalize(base)) {
                hints.insert(d.name.clone(), format!("type stubs for `{base}`"));
                continue;
            }
        }
        // Ships an executable -> used via CLI / npm scripts.
        if let Some(bin) = pkg_bin(root, &d.raw_name) {
            hints.insert(d.name.clone(), format!("provides executable `{bin}`"));
            continue;
        }
        // Named in a config file or npm script.
        if contains_token(&config_blob, &d.raw_name) {
            hints.insert(d.name.clone(), "referenced in config / scripts".into());
            continue;
        }
        // Naming conventions for plugins loaded by string.
        if let Some(reason) = name_convention(&d.raw_name) {
            hints.insert(d.name.clone(), reason);
        }
    }
    hints
}

/// The `bin` a package ships, if any (from its installed package.json).
fn pkg_bin(root: &Path, raw: &str) -> Option<String> {
    let pj = root.join("node_modules").join(raw).join("package.json");
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(pj).ok()?).ok()?;
    match v.get("bin")? {
        serde_json::Value::String(_) => Some(raw.rsplit('/').next().unwrap_or(raw).to_string()),
        serde_json::Value::Object(o) => o.keys().next().cloned(),
        _ => None,
    }
}

/// npm scripts + embedded package.json config sections + root config files.
fn config_blob(root: &Path) -> String {
    let mut blob = String::new();
    if let Ok(txt) = std::fs::read_to_string(root.join("package.json")) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
            if let Some(scripts) = v.get("scripts").and_then(|x| x.as_object()) {
                for val in scripts.values() {
                    if let Some(s) = val.as_str() {
                        blob.push_str(s);
                        blob.push('\n');
                    }
                }
            }
            for key in [
                "eslintConfig", "prettier", "jest", "babel", "husky", "lint-staged", "postcss",
                "mocha", "nyc", "commitlint", "stylelint", "release", "standard", "c8",
            ] {
                if let Some(sec) = v.get(key) {
                    blob.push_str(&sec.to_string());
                    blob.push('\n');
                }
            }
        }
    }
    for f in CONFIG_FILES {
        if let Ok(txt) = std::fs::read_to_string(root.join(f)) {
            blob.push_str(&txt);
            blob.push('\n');
        }
    }
    blob
}

fn name_convention(raw: &str) -> Option<String> {
    const PATTERNS: &[(&str, &str)] = &[
        ("eslint-plugin-", "eslint plugin"),
        ("eslint-config-", "eslint config"),
        ("@babel/preset-", "babel preset"),
        ("@babel/plugin-", "babel plugin"),
        ("stylelint-config-", "stylelint config"),
        ("@commitlint/", "commitlint package"),
    ];
    for (p, label) in PATTERNS {
        if raw.starts_with(p) {
            return Some(format!("name matches `{p}*` ({label})"));
        }
    }
    if raw.contains("/eslint-plugin") {
        return Some("scoped eslint plugin".into());
    }
    None
}

/// Turn an import specifier into a package name, or None for relative /
/// builtin / non-package imports.
fn spec_to_package(spec: &str) -> Option<String> {
    if spec.starts_with('.') || spec.starts_with('/') {
        return None;
    }
    let s = spec.strip_prefix("node:").unwrap_or(spec);
    let root = if let Some(scoped) = s.strip_prefix('@') {
        let mut it = scoped.splitn(2, '/');
        let scope = it.next()?;
        let pkg = it.next()?.split('/').next()?;
        format!("@{}/{}", scope, pkg)
    } else {
        s.split('/').next()?.to_string()
    };
    if is_builtin(&root) {
        return None;
    }
    Some(normalize(&root))
}

fn is_builtin(name: &str) -> bool {
    const BUILTINS: &[&str] = &[
        "assert", "buffer", "child_process", "cluster", "console", "constants", "crypto",
        "dgram", "dns", "domain", "events", "fs", "http", "http2", "https", "inspector",
        "module", "net", "os", "path", "perf_hooks", "process", "punycode", "querystring",
        "readline", "repl", "stream", "string_decoder", "sys", "timers", "tls", "trace_events",
        "tty", "url", "util", "v8", "vm", "wasi", "worker_threads", "zlib",
    ];
    BUILTINS.contains(&name)
}
