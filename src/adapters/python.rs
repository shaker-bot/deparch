//! Python adapter.
//! declared: pyproject.toml / requirements.txt  ·  installed + import→package
//! map: importlib.metadata (via parse_py.py)  ·  imports: ast (via parse_py.py)

use super::Adapter;
use crate::model::*;
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub struct PythonAdapter;

const SCRIPT: &str = include_str!("../scripts/parse_py.py");

#[derive(serde::Deserialize)]
struct PyOut {
    imports: Vec<RawImport>,
    module_to_dist: HashMap<String, Vec<String>>,
    installed: Vec<PyInstalled>,
}

#[derive(serde::Deserialize)]
struct RawImport {
    specifier: String,
    file: String,
    line: usize,
}

#[derive(serde::Deserialize)]
struct PyInstalled {
    name: String,
    norm: String,
    version: String,
    requires: Vec<String>,
    #[serde(default)]
    entry_groups: Vec<String>,
}

impl Adapter for PythonAdapter {
    fn language(&self) -> &'static str {
        "python"
    }

    fn detect(&self, root: &Path) -> bool {
        ["pyproject.toml", "requirements.txt", "setup.py", "setup.cfg", "Pipfile"]
            .iter()
            .any(|f| root.join(f).exists())
    }

    fn analyze(&self, root: &Path) -> Result<Analysis> {
        let python = project_python(root);
        let raw =
            super::run_script(SCRIPT, "py", &python, root).context("running python parser")?;
        let out: PyOut = serde_json::from_str(&raw).context("parsing python parser output")?;

        let mut installed = HashMap::new();
        let mut entry_groups: HashMap<String, Vec<String>> = HashMap::new();
        for i in out.installed {
            entry_groups.insert(i.norm.clone(), i.entry_groups);
            installed.insert(
                i.norm,
                ResolvedPkg {
                    name: i.name,
                    version: i.version,
                    requires: i.requires,
                },
            );
        }

        // import module name -> distribution package name (the Python-specific moat)
        let mut mod_map: HashMap<String, String> = HashMap::new();
        for (module, dists) in &out.module_to_dist {
            if let Some(first) = dists.first() {
                mod_map.insert(module.clone(), normalize(first));
            }
        }

        let mut used = Vec::new();
        for imp in out.imports {
            if let Some(pkg) = mod_map.get(&imp.specifier) {
                used.push(Usage {
                    package: pkg.clone(),
                    import: SourceImport {
                        specifier: imp.specifier,
                        file: imp.file,
                        line: imp.line,
                    },
                });
            }
        }

        let declared = read_declared(root)?;
        let usage_hints = compute_hints(root, &declared, &entry_groups);

        Ok(Analysis {
            language: "python".into(),
            declared,
            installed,
            used,
            usage_hints,
        })
    }
}

/// Config/build files that reference tools by name rather than importing them.
const CONFIG_FILES: &[&str] = &[
    "setup.cfg", "tox.ini", ".pre-commit-config.yaml", ".flake8", "mypy.ini", "pytest.ini",
    ".isort.cfg", "noxfile.py", "Makefile", ".bandit", ".pylintrc", ".coveragerc",
];

/// Entry-point groups that indicate a package is invoked, not imported.
const ACTIVE_GROUPS: &[&str] = &["console_scripts", "gui_scripts", "pytest11"];

fn compute_hints(
    root: &Path,
    declared: &[DeclaredDep],
    entry_groups: &HashMap<String, Vec<String>>,
) -> HashMap<String, String> {
    let build_reqs = build_system_reqs(root);
    let tool_sections = tool_sections(root);
    let config_blob = config_blob(root);

    let mut hints = HashMap::new();
    for d in declared {
        // Stub-only distributions (types-requests, ...) feed the type checker.
        if let Some(base) = d.raw_name.strip_prefix("types-") {
            hints.insert(d.name.clone(), format!("type stubs for `{base}`"));
            continue;
        }
        // Provides a console script or registers as a pytest plugin.
        if let Some(groups) = entry_groups.get(&d.name) {
            if let Some(g) = groups.iter().find(|g| ACTIVE_GROUPS.contains(&g.as_str())) {
                let what = if g == "pytest11" { "pytest plugin" } else { "CLI tool" };
                hints.insert(d.name.clone(), format!("{what} (entry point `{g}`)"));
                continue;
            }
        }
        // Build backend / build-time requirement.
        if build_reqs.contains(&d.name) {
            hints.insert(d.name.clone(), "build-system requirement".into());
            continue;
        }
        // Configured via a [tool.<name>] section.
        if tool_sections.contains(&d.name) {
            hints.insert(d.name.clone(), format!("configured in [tool.{}]", d.raw_name));
            continue;
        }
        // Referenced in a config file (pre-commit, tox, setup.cfg, ...).
        if contains_token(&config_blob, &d.raw_name) {
            hints.insert(d.name.clone(), "referenced in config".into());
            continue;
        }
        // Naming conventions for plugins.
        if let Some(reason) = name_convention(&d.name) {
            hints.insert(d.name.clone(), reason);
        }
    }
    hints
}

fn build_system_reqs(root: &Path) -> HashSet<String> {
    let mut set = HashSet::new();
    let txt = match std::fs::read_to_string(root.join("pyproject.toml")) {
        Ok(t) => t,
        Err(_) => return set,
    };
    let Ok(val) = txt.parse::<toml::Value>() else {
        return set;
    };
    if let Some(arr) = val
        .get("build-system")
        .and_then(|b| b.get("requires"))
        .and_then(|r| r.as_array())
    {
        for item in arr {
            if let Some(s) = item.as_str() {
                let name: String = s
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
                    .collect();
                set.insert(normalize(&name));
            }
        }
    }
    set
}

/// Normalized names of every `[tool.<name>]` section in pyproject.toml.
fn tool_sections(root: &Path) -> HashSet<String> {
    let mut set = HashSet::new();
    if let Ok(txt) = std::fs::read_to_string(root.join("pyproject.toml")) {
        if let Ok(val) = txt.parse::<toml::Value>() {
            if let Some(tbl) = val.get("tool").and_then(|t| t.as_table()) {
                for key in tbl.keys() {
                    set.insert(normalize(key));
                }
            }
        }
    }
    set
}

fn config_blob(root: &Path) -> String {
    let mut blob = String::new();
    for f in CONFIG_FILES {
        if let Ok(txt) = std::fs::read_to_string(root.join(f)) {
            blob.push_str(&txt);
            blob.push('\n');
        }
    }
    blob
}

fn name_convention(name: &str) -> Option<String> {
    const PATTERNS: &[(&str, &str)] = &[
        ("pytest-", "pytest plugin"),
        ("flake8-", "flake8 plugin"),
        ("sphinxcontrib-", "sphinx extension"),
        ("pylint-", "pylint plugin"),
    ];
    for (p, label) in PATTERNS {
        if name.starts_with(p) {
            return Some(format!("name matches `{p}*` ({label})"));
        }
    }
    None
}

/// Prefer the project's own interpreter so `importlib.metadata` reflects the
/// project's installed packages, not whatever `python3` is on PATH.
fn project_python(root: &Path) -> String {
    if let Ok(venv) = std::env::var("VIRTUAL_ENV") {
        let p = Path::new(&venv).join("bin/python");
        if p.exists() {
            return p.to_string_lossy().into_owned();
        }
    }
    for candidate in [".venv/bin/python", "venv/bin/python", "env/bin/python"] {
        let p = root.join(candidate);
        if p.exists() {
            return p.to_string_lossy().into_owned();
        }
    }
    "python3".to_string()
}

fn read_declared(root: &Path) -> Result<Vec<DeclaredDep>> {
    let mut deps = Vec::new();

    let pyproject = root.join("pyproject.toml");
    if pyproject.exists() {
        let txt = std::fs::read_to_string(&pyproject)?;
        if let Ok(val) = txt.parse::<toml::Value>() {
            // PEP 621: [project].dependencies (array of PEP 508 strings)
            if let Some(arr) = val
                .get("project")
                .and_then(|p| p.get("dependencies"))
                .and_then(|d| d.as_array())
            {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        push_req(&mut deps, s, DepKind::Prod, "pyproject.toml");
                    }
                }
            }
            // PEP 621: [project.optional-dependencies]
            if let Some(tbl) = val
                .get("project")
                .and_then(|p| p.get("optional-dependencies"))
                .and_then(|d| d.as_table())
            {
                for arr in tbl.values() {
                    if let Some(arr) = arr.as_array() {
                        for item in arr {
                            if let Some(s) = item.as_str() {
                                push_req(&mut deps, s, DepKind::Optional, "pyproject.toml");
                            }
                        }
                    }
                }
            }
            // Poetry: [tool.poetry.dependencies] (table keyed by name)
            if let Some(tbl) = val
                .get("tool")
                .and_then(|t| t.get("poetry"))
                .and_then(|p| p.get("dependencies"))
                .and_then(|d| d.as_table())
            {
                for name in tbl.keys() {
                    if name.eq_ignore_ascii_case("python") {
                        continue;
                    }
                    push_req(&mut deps, name, DepKind::Prod, "pyproject.toml");
                }
            }
        }
    }

    let reqs = root.join("requirements.txt");
    if reqs.exists() {
        let txt = std::fs::read_to_string(&reqs)?;
        for line in txt.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('-') {
                continue;
            }
            push_req(&mut deps, line, DepKind::Prod, "requirements.txt");
        }
    }

    Ok(deps)
}

/// Extract the bare package name from a PEP 508 requirement string.
fn push_req(deps: &mut Vec<DeclaredDep>, spec: &str, kind: DepKind, manifest: &str) {
    let name: String = spec
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
        .collect();
    let name = name.trim();
    if name.is_empty() {
        return;
    }
    deps.push(DeclaredDep {
        name: normalize(name),
        raw_name: name.to_string(),
        kind,
        manifest: manifest.to_string(),
    });
}
