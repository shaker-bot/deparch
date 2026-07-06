//! Language-agnostic normalized model that every adapter produces.

use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum DepKind {
    Prod,
    Dev,
    Optional,
    Peer,
}

/// A dependency the project *declares* in a manifest.
#[derive(Debug, Clone, Serialize)]
pub struct DeclaredDep {
    pub name: String, // normalized
    pub raw_name: String,
    pub kind: DepKind,
    pub manifest: String,
}

/// A package that is actually *installed*, with who it requires.
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedPkg {
    pub name: String, // display name
    pub version: String,
    pub requires: Vec<String>, // normalized names
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceImport {
    pub specifier: String,
    pub file: String,
    pub line: usize,
}

/// One resolved use of a package in source code.
#[derive(Debug, Clone, Serialize)]
pub struct Usage {
    pub package: String, // normalized package name
    pub import: SourceImport,
}

/// Everything one adapter knows about one ecosystem in the project.
#[derive(Debug, Clone, Serialize)]
pub struct Analysis {
    pub language: String,
    pub declared: Vec<DeclaredDep>,
    pub installed: HashMap<String, ResolvedPkg>, // keyed by normalized name
    pub used: Vec<Usage>,
    /// normalized package name -> reason it's likely used without a source
    /// import (binary, config reference, entry point, type stubs, ...).
    pub usage_hints: HashMap<String, String>,
}

/// Whole-token containment: is `needle` present in `hay` not embedded inside a
/// larger package-like identifier? Used to scan config files/scripts for a
/// package name without matching `chalk` inside `chalk-next`.
pub fn contains_token(hay: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let bytes = hay.as_bytes();
    let nlen = needle.len();
    let is_word = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'@' | b'/');
    let mut start = 0;
    while let Some(pos) = hay[start..].find(needle) {
        let i = start + pos;
        let before_ok = i == 0 || !is_word(bytes[i - 1]);
        let after = i + nlen;
        let after_ok = after >= bytes.len() || !is_word(bytes[after]);
        if before_ok && after_ok {
            return true;
        }
        start = i + 1;
    }
    false
}

/// PEP 503 / npm-friendly name normalization so declared, installed, and
/// imported names can be compared across the three data sources.
pub fn normalize(name: &str) -> String {
    let mut out = String::new();
    let mut prev_sep = false;
    for c in name.trim().chars() {
        if c == '-' || c == '_' || c == '.' {
            if !prev_sep {
                out.push('-');
                prev_sep = true;
            }
        } else {
            out.push(c.to_ascii_lowercase());
            prev_sep = false;
        }
    }
    out.trim_matches('-').to_string()
}
