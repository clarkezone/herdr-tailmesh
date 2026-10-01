fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto = "../api/proto/agentflow/v1/control.proto";
    println!("cargo:rerun-if-changed={proto}");
    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc_bin_vendored::protoc_bin_path()?);
    // NodeControl has an RPC named Connect; omit tonic's convenience constructor
    // of the same name. Clients are constructed from explicit channels below.
    tonic_prost_build::configure()
        .build_transport(false)
        .compile_with_config(config, &[proto], &["../api/proto"])?;
    Ok(())
}
