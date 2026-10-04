//! ontoctl — Onto Assurance Kernel CLI
//!
//! Commands:
//!   ontoctl classify <kind> [args]     Classify a capability's EffectClass
//!   ontoctl verify <fixture>            Run differential verification
//!   ontoctl hash <input>                Compute canonical hash
//!   ontoctl profile <workspace>         Profile a code project

use onto_assurance_core::canonical;
use onto_assurance_core::effect_classifier::{EffectClassifier, RulesBasedClassifier};
use onto_assurance_types::hash::{HashDomain, HashPurpose};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: ontoctl <classify|hash|profile> ...");
        std::process::exit(1);
    }

    match args[1].as_str() {
        "classify" => cmd_classify(&args),
        "hash" => cmd_hash(&args),
        "profile" => cmd_profile(&args),
        _ => {
            eprintln!("Unknown command: {}", args[1]);
            std::process::exit(1);
        }
    }
}

fn cmd_classify(args: &[String]) {
    let kind = args.get(2).map(|s| s.as_str()).unwrap_or("unknown");
    let hints = args.get(3).map(|s| s.as_str()).unwrap_or("");
    let classifier = RulesBasedClassifier::new();
    let result = classifier.classify(
        kind, hints, None,
        onto_assurance_types::enums::RiskLevel::Medium,
    );
    match serde_json::to_string_pretty(&result) {
        Ok(json) => println!("{}", json),
        Err(e) => { eprintln!("serialization error: {}", e); std::process::exit(1); }
    }
}

fn cmd_hash(args: &[String]) {
    let input = args.get(2).map(|s| s.as_str()).unwrap_or("");
    let domain = HashDomain::new(HashPurpose::Content, "CLI");
    match canonical::compute_hash(&input, &domain) {
        Ok(h) => println!("{}", hex::encode(h)),
        Err(e) => { eprintln!("hash error: {}", e); std::process::exit(1); }
    }
}

fn cmd_profile(args: &[String]) {
    let files: Vec<String> = args.iter().skip(2).cloned().collect();
    if files.is_empty() {
        eprintln!("Usage: ontoctl profile <file1> [file2...]");
        std::process::exit(1);
    }
    let profile = onto_code_pack::project_profiler::profile_project(&files);
    match serde_json::to_string_pretty(&profile) {
        Ok(json) => println!("{}", json),
        Err(e) => { eprintln!("serialization error: {}", e); std::process::exit(1); }
    }
}
