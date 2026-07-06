# deparch — dependency archaeology

Cross-language dependency analysis. Answers two questions the native tools only half-answer, in one unified report across ecosystems:

- **unused** — declared in a manifest, never imported in source
- **phantom** — imported in source, never declared (you're leaning on a transitive)
- **why** — reverse-walk the resolved tree to show what pulled a package in

Supports **Node/TypeScript** and **Python** today. Adding a language = one adapter.

## Usage

```
deparch --path <dir> check           # unused + phantom, all detected ecosystems
deparch --path <dir> check --strict  # also list deps used without a source import
deparch --path <dir> check --json    # machine-readable
deparch --path <dir> why <package>   # dependency chain(s) for a package
```

## Unused-dependency accuracy

A dependency that is never `import`ed is not necessarily unused — it may be used
via a **binary** (`tsc`, `black`), a **config-file reference** (eslint/babel
plugins, `[tool.black]`), a **build backend**, an **entry point** (console
scripts, pytest plugins), or **type stubs** (`@types/*`, `types-*`). Those are
detected and separated from confidently-unused deps, so the default report
stays low-noise. `--strict` lists them with the reason each was spared.

| signal | Node/TS | Python |
|--------|---------|--------|
| ships an executable | `bin` in installed package.json | `console_scripts` / `gui_scripts` entry point |
| plugin registration | naming (`eslint-plugin-*`) | `pytest11` entry point, `pytest-*` naming |
| config reference | `.eslintrc`, `babel.config.*`, npm scripts, ... | `setup.cfg`, `tox.ini`, `.pre-commit-config.yaml`, `[tool.*]` |
| build tooling | — | `[build-system].requires` |
| type stubs | `@types/x` (used when `x` is) | `types-x` |

## How it works

Each ecosystem has an **adapter** that produces a normalized `Analysis`
(declared deps · installed tree · resolved source imports). A language-agnostic
**engine** cross-references those three; a **reporter** renders it.

The hard part — and the value over single-language tools — is mapping an
**import name back to a package name**. Trivial in Node (`lodash/fp` → `lodash`),
not in Python (`import yaml` → `PyYAML`, `import bs4` → `beautifulsoup4`), which
requires installed metadata.

| ecosystem | declared | installed tree | imports |
|-----------|----------|----------------|---------|
| Node/TS   | `package.json` | `package-lock.json` | TS compiler API / regex fallback |
| Python    | `pyproject.toml`, `requirements.txt` | `importlib.metadata` | `ast` |

Python analysis reads the **project's** environment: `$VIRTUAL_ENV`, then
`.venv`/`venv`/`env`, then `python3` on PATH.

## Requirements

`node` and `python3` must be available for their respective adapters (native
per-language parsing). Roadmap: swap in Rust tree-sitter to drop the runtime
dependency and add languages faster.

## Known limitations (v0)

- `why` uses lockfile/metadata edges without version-range resolution.
- Config-reference detection is a token scan, not a semantic parse of each
  config format — it can over-suppress if a package name appears incidentally.
- Naming-convention hints (`eslint-plugin-*`, `pytest-*`) are lower-confidence
  than binary/entry-point/config signals.
