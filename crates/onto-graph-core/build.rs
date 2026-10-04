//! Compile the ofg-core C kernel.
//!
//! Set OFG_CBM_ROOT to the CBM source tree if not at the default location.

fn main() {
    let cbm_root = std::env::var("OFG_CBM_ROOT")
        .unwrap_or_else(|_| "../codebase-memory-mcp-main".into());

    let include_path = format!("{cbm_root}/src");
    let ofg_dir = "ofg-core/src/ofg";
    let pipeline_dir = format!("{cbm_root}/src/pipeline");
    let foundation_dir = format!("{cbm_root}/src/foundation");
    let internal_dir = format!("{cbm_root}/internal/cbm");
    let vendored_dir = format!("{cbm_root}/vendored");

    // Only compile if the CBM source tree exists
    if !std::path::Path::new(&include_path).exists() {
        println!("cargo:warning=CBM source not found at {cbm_root} — skipping C kernel compilation");
        println!("cargo:warning=Set OFG_CBM_ROOT to the CBM repository path to enable C kernel");
        return;
    }

    cc::Build::new()
        .file(format!("{ofg_dir}/ofg_core.c"))
        .include(ofg_dir)
        .include(&include_path)
        .include(&pipeline_dir)
        .include(&foundation_dir)
        .include(&internal_dir)
        .include(vendored_dir)
        .compile("ofg_core");

    println!("cargo:rerun-if-changed={ofg_dir}/ofg_core.c");
    println!("cargo:rerun-if-changed={ofg_dir}/ofg_core.h");
    println!("cargo:rerun-if-changed={ofg_dir}/ofg_graph_sink.h");
    println!("cargo:rerun-if-env-changed=OFG_CBM_ROOT");
}
