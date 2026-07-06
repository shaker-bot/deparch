//! Human-readable rendering of findings.

use crate::engine::Finding;

pub fn print_findings(findings: &[Finding], strict: bool) {
    let mut noted = false;
    for f in findings {
        println!("\n=== {} ===", f.language);
        let nothing = f.unused.is_empty() && f.phantom.is_empty() && (f.suppressed.is_empty() || !strict);
        if nothing {
            if f.suppressed.is_empty() {
                println!("  clean — no unused or phantom deps found");
            } else {
                println!(
                    "  clean — {} dep(s) not imported but likely used (--strict to list)",
                    f.suppressed.len()
                );
            }
            continue;
        }
        if !f.unused.is_empty() {
            println!("  unused (declared, never imported):");
            for u in &f.unused {
                println!("    ✗ {:<26} [{:?}]  {}", u.name, u.kind, u.manifest);
            }
        }
        if !f.phantom.is_empty() {
            println!("  phantom (imported, not declared — relying on a transitive):");
            for p in &f.phantom {
                println!(
                    "    ! {:<26} {}  (e.g. {}:{})",
                    p.name, p.version, p.example_file, p.example_line
                );
            }
        }
        if strict && !f.suppressed.is_empty() {
            println!("  not imported, but likely used:");
            for s in &f.suppressed {
                println!("    ~ {:<26} [{:?}]  {}", s.name, s.kind, s.reason);
            }
        } else if !f.suppressed.is_empty() {
            noted = true;
            println!(
                "  ({} dep(s) not imported but likely used — --strict to list)",
                f.suppressed.len()
            );
        }
    }
    if noted {
        println!(
            "\nnote: 'likely used' = a binary, config reference, entry point, or type\n      stub explains the dependency despite no source import."
        );
    }
}

pub fn print_why(lang: &str, target: &str, chains: &[Vec<String>]) {
    println!("\n[{}] why is '{}' here:", lang, target);
    for chain in chains {
        println!("  {}", chain.join(" → "));
    }
}
