"""Emit imports, module->distribution map, and installed dists as JSON.
Run within the project's venv so importlib.metadata sees the right packages.
Usage: python3 parse_py.py <root>"""
import ast
import json
import os
import re
import sys
from importlib import metadata

root = sys.argv[1]
IGNORE = {
    ".git", "node_modules", ".venv", "venv", "env", "__pycache__", ".tox",
    "build", "dist", ".mypy_cache", ".pytest_cache", ".ruff_cache", ".eggs",
}


def norm(n):
    return re.sub(r"[-_.]+", "-", n.strip()).lower()


imports = []
for dirpath, dirnames, filenames in os.walk(root):
    dirnames[:] = [d for d in dirnames if d not in IGNORE and not d.startswith(".")]
    for fn in filenames:
        if not fn.endswith((".py", ".pyi")):
            continue
        path = os.path.join(dirpath, fn)
        try:
            with open(path, "r", encoding="utf-8") as f:
                tree = ast.parse(f.read(), filename=path)
        except (SyntaxError, UnicodeDecodeError, OSError):
            continue
        rel = os.path.relpath(path, root)
        for node in ast.walk(tree):
            if isinstance(node, ast.Import):
                for a in node.names:
                    imports.append(
                        {"specifier": a.name.split(".")[0], "file": rel, "line": node.lineno}
                    )
            elif isinstance(node, ast.ImportFrom):
                if node.level and node.level > 0:
                    continue  # relative import -> first-party
                if node.module:
                    imports.append(
                        {"specifier": node.module.split(".")[0], "file": rel, "line": node.lineno}
                    )

try:
    module_to_dist = {k: list(v) for k, v in metadata.packages_distributions().items()}
except Exception:
    module_to_dist = {}

installed = []
for dist in metadata.distributions():
    try:
        name = dist.metadata["Name"]
    except Exception:
        name = None
    if not name:
        continue
    reqs = []
    for r in (dist.requires or []):
        base = r.split(";")[0].strip()
        m = re.match(r"^([A-Za-z0-9_.\-]+)", base)
        if m:
            reqs.append(norm(m.group(1)))
    groups = set()
    try:
        for ep in dist.entry_points:
            groups.add(ep.group)
    except Exception:
        pass
    installed.append(
        {
            "name": name,
            "norm": norm(name),
            "version": dist.version or "",
            "requires": reqs,
            "entry_groups": sorted(groups),
        }
    )

json.dump(
    {"imports": imports, "module_to_dist": module_to_dist, "installed": installed},
    sys.stdout,
)
