fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Archive builds can lack git; report that honestly rather than stamping main.
    fn git(args: &[&str]) -> Option<String> {
        let output = std::process::Command::new("git").args(args).output().ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }
    let commit = git(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let dirty = git(&["status", "--porcelain", "--untracked-files=no"])
        .map_or("unknown", |s| if s.is_empty() { "false" } else { "true" });
    println!("cargo:rustc-env=HERDR_VISUALIZER_GIT_COMMIT={commit}");
    println!("cargo:rustc-env=HERDR_VISUALIZER_GIT_DIRTY={dirty}");
    for path in ["src", "Cargo.toml", "Cargo.lock", "build.rs"] {
        println!("cargo:rerun-if-changed={path}");
    }
    for name in ["HEAD".to_owned(), "index".to_owned()]
        .into_iter()
        .chain(git(&["symbolic-ref", "-q", "HEAD"]))
    {
        if let Some(path) = git(&["rev-parse", "--path-format=absolute", "--git-path", &name]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
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
