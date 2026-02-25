fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_files = &[
        "../protos/postgres_service.proto",
        "../protos/influxdb_service.proto",
    ];
    let include_dirs = &["../protos"];

    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        // Add serde derives to every generated message so they can be
        // serialised directly to JSON in HTTP responses.
        .type_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]")
        .compile_protos(proto_files, include_dirs)?;

    // Re-run if any proto file changes.
    for file in proto_files {
        println!("cargo:rerun-if-changed={file}");
    }

    Ok(())
}
