//! Per-ecosystem adapters. Each turns a project directory into a normalized
//! `Analysis`. Adding a language = adding one adapter here.

pub mod node;
pub mod python;

use crate::model::Analysis;
use anyhow::Result;
use std::path::Path;

pub trait Adapter {
    fn language(&self) -> &'static str;
    fn detect(&self, root: &Path) -> bool;
    fn analyze(&self, root: &Path) -> Result<Analysis>;
}

pub fn all() -> Vec<Box<dyn Adapter>> {
    vec![Box::new(node::NodeAdapter), Box::new(python::PythonAdapter)]
}

/// Write an embedded parser script to a temp file and run it as
/// `<cmd> <script> <root>`, returning stdout.
pub(crate) fn run_script(script: &str, ext: &str, cmd: &str, root: &Path) -> Result<String> {
    let mut tmp = std::env::temp_dir();
    tmp.push(format!("deparch_{}_{}.{}", std::process::id(), ext, ext));
    std::fs::write(&tmp, script)?;
    let out = std::process::Command::new(cmd).arg(&tmp).arg(root).output();
    let _ = std::fs::remove_file(&tmp);
    let out = out?;
    if !out.status.success() {
        anyhow::bail!(
            "{} parser failed: {}",
            cmd,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
