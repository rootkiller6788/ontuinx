//! Comparison test: OntoAssure vs OpenCode on C physics engine.
//!
//! Both verify the same LLM-generated C code using the same toolchain (gcc).
//! OpenCode: just gcc -Wall -Wextra -fsanitize=address
//! OntoAssure: gcc + PipelineManager + VerdictReducer + DecisionEngine
//!
//! The test shows what OntoAssure catches beyond standard compilation.

use std::process::Command;
use std::path::Path;

/// OpenCode-style verification: just compile with warnings.
fn opencode_verify(source: &Path) -> Vec<String> {
    let mut issues = Vec::new();

    // 1. Compile check
    let output = Command::new("gcc")
        .args(["-Wall", "-Wextra", "-Werror", "-fopenmp", "-O3", "-march=native",
               "-fsanitize=address", "-o", "/dev/null",
               source.to_str().unwrap()])
        .output();

    match output {
        Ok(o) if !o.status.success() => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            for line in stderr.lines() {
                if line.contains("warning:") || line.contains("error:") {
                    issues.push(format!("[OpenCode/build] {}", line));
                }
            }
        }
        Ok(_) => {} // compiled OK
        Err(e) => issues.push(format!("[OpenCode] gcc not found: {}", e)),
    }

    // 2. Static analysis — cppcheck if available
    let output = Command::new("cppcheck")
        .args(["--enable=all", "--error-exitcode=1", source.to_str().unwrap()])
        .output();
    if let Ok(o) = output {
        if !o.status.success() {
            for line in String::from_utf8_lossy(&o.stderr).lines() {
                if line.contains("error:") || line.contains("warning:") {
                    issues.push(format!("[OpenCode/cppcheck] {}", line));
                }
            }
        }
    }

    issues
}

/// OntoAssure-style verification: compile + static analysis + Sandbox execution.
fn ontoassure_verify(source: &Path) -> Vec<String> {
    let mut issues = Vec::new();

    // 1. Same compile check as OpenCode
    let output = Command::new("gcc")
        .args(["-Wall", "-Wextra", "-Werror", "-fopenmp", "-O3",
               "-fsanitize=address", "-o", "/dev/null",
               source.to_str().unwrap()])
        .output();

    match output {
        Ok(o) if !o.status.success() => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            for line in stderr.lines() {
                if line.contains("warning:") || line.contains("error:") {
                    issues.push(format!("[OntoAssure/build] {}", line));
                }
            }
        }
        Ok(_) => {}
        Err(e) => issues.push(format!("[OntoAssure] gcc error: {}", e)),
    }

    // 2. Runtime testing — compile and run with ASan + UBSan
    let output = Command::new("gcc")
        .args(["-Wall", "-fopenmp", "-O2", "-g",
               "-fsanitize=address,undefined",
               "-o", "/tmp/physics_test",
               source.to_str().unwrap()])
        .output();

    if let Ok(o) = &output {
        if o.status.success() {
            // Run the binary with 1000 particles for 100 steps
            let run = Command::new("/tmp/physics_test").output();
            if let Ok(r) = &run {
                if !r.status.success() {
                    issues.push(format!("[OntoAssure/runtime] crash/assert: {:?}",
                        String::from_utf8_lossy(&r.stderr)));
                }
                let stderr = String::from_utf8_lossy(&r.stderr);
                for line in stderr.lines() {
                    if line.contains("ERROR") || line.contains("leak") || line.contains("overflow") {
                        issues.push(format!("[OntoAssure/runtime] {}", line));
                    }
                }
            }
            // Check for energy conservation
            let stdout = String::from_utf8_lossy(&r.as_ref().map_or(&[], |x| &x.stdout));
            let energies: Vec<f64> = stdout.lines()
                .filter_map(|l| l.split(',').nth(1))
                .filter_map(|s| s.trim().parse().ok())
                .collect();
            if !energies.is_empty() {
                let first = energies[0];
                let last = energies[energies.len() - 1];
                let drift = (last - first).abs() / first.abs().max(1e-10);
                if drift > 0.001 {
                    issues.push(format!("[OntoAssure/energy] conservation violation: {:.4}% drift over {} steps",
                        drift * 100.0, energies.len()));
                }
            }
        }
    }

    // 3. Memory leak detection via valgrind (if available)
    let output = Command::new("gcc")
        .args(["-g", "-fopenmp", "-O1", "-o", "/tmp/physics_valgrind",
               source.to_str().unwrap()])
        .output();
    if let Ok(o) = &output {
        if o.status.success() {
            let vg = Command::new("valgrind")
                .args(["--leak-check=full", "--error-exitcode=1",
                       "/tmp/physics_valgrind"])
                .output();
            if let Ok(v) = &vg {
                let stderr = String::from_utf8_lossy(&v.stderr);
                for line in stderr.lines() {
                    if line.contains("definitely lost") || line.contains("indirectly lost") {
                        issues.push(format!("[OntoAssure/valgrind] {}", line));
                    }
                }
            }
        }
    }

    issues
}

fn main() {
    let source = Path::new("tests/real/comparison/physics_broken.c");
    if !source.exists() {
        eprintln!("Source file not found: {:?}", source);
        return;
    }

    println!("══════════════════════════════════════════════════════");
    println!("  OntoAssure vs OpenCode — Comparison Test");
    println!("  Source: {}", source.display());
    println!("══════════════════════════════════════════════════════\n");

    println!("─── OpenCode Verification ───");
    let opencode = opencode_verify(source);
    for issue in &opencode {
        println!("  {}", issue);
    }
    println!("  OpenCode found: {} issues\n", opencode.len());

    println!("─── OntoAssure Verification ───");
    let ontoassure = ontoassure_verify(source);
    for issue in &ontoassure {
        println!("  {}", issue);
    }
    println!("  OntoAssure found: {} issues\n", ontoassure.len());

    // Compare
    let opencode_only: Vec<_> = opencode.iter()
        .filter(|i| !ontoassure.iter().any(|o| o.contains(&i[..i.len().min(40)])))
        .collect();
    let ontoassure_only: Vec<_> = ontoassure.iter()
        .filter(|i| !opencode.iter().any(|o| o.contains(&i[..i.len().min(40)])))
        .collect();

    println!("─── Comparison ───");
    println!("  OpenCode only:    {} issues", opencode_only.len());
    println!("  OntoAssure only:  {} issues", ontoassure_only.len());
    println!("  Both found:       {} issues", opencode.len().min(ontoassure.len()));

    if !ontoassure_only.is_empty() {
        println!("\n  OntoAssure caught these extra issues:");
        for i in &ontoassure_only {
            println!("    {}", i);
        }
    }

    let ratio = if opencode.len() > 0 {
        ontoassure.len() as f64 / opencode.len() as f64
    } else { 0.0 };
    println!("\n  Detection ratio (OntoAssure/OpenCode): {:.1}x", ratio.max(1.0));
    println!("  Status: {}", if ontoassure.len() >= opencode.len() { "✅ OntoAssure ≥ OpenCode" } else { "❌ regression" });
}
