mod adapters;
mod engine;
mod model;
mod report;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "deparch", version, about = "Cross-language dependency archaeology")]
struct Cli {
    /// Project directory to analyze.
    #[arg(long, global = true, default_value = ".")]
    path: PathBuf,
    /// Emit machine-readable JSON.
    #[arg(long, global = true)]
    json: bool,
    /// Also list deps that are likely used without a source import.
    #[arg(long, global = true)]
    strict: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Full report: unused + phantom deps across every detected ecosystem.
    Check,
    /// Explain why a package is installed (its dependency chain).
    Why { package: String },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let root = &cli.path;

    let active: Vec<_> = adapters::all()
        .into_iter()
        .filter(|a| a.detect(root))
        .collect();

    if active.is_empty() {
        eprintln!(
            "no supported ecosystem detected in {} (looked for package.json, pyproject.toml, requirements.txt, ...)",
            root.display()
        );
        std::process::exit(1);
    }

    match &cli.cmd {
        Cmd::Check => {
            let mut findings = Vec::new();
            for a in &active {
                match a.analyze(root) {
                    Ok(an) => findings.push(engine::analyze(&an)),
                    Err(e) => eprintln!("[{}] skipped: {:#}", a.language(), e),
                }
            }
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&findings)?);
            } else {
                report::print_findings(&findings, cli.strict);
            }
        }
        Cmd::Why { package } => {
            let mut found = false;
            for a in &active {
                match a.analyze(root) {
                    Ok(an) => {
                        let chains = engine::why(&an, package);
                        if !chains.is_empty() {
                            found = true;
                            report::print_why(a.language(), package, &chains);
                        }
                    }
                    Err(e) => eprintln!("[{}] skipped: {:#}", a.language(), e),
                }
            }
            if !found {
                eprintln!("'{}' not found in any resolved dependency tree", package);
            }
        }
    }

    Ok(())
}
