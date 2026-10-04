fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .compile_protos(
            &["proto/health.proto",
              "proto/snapshot_query.proto",
              "proto/graph_risk.proto",
              "proto/query.proto"],
            &["proto"],
        )?;
    Ok(())
}
