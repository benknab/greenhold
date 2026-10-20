use anyhow::{Context as _, Result, bail};
use risc0_build::{DockerOptionsBuilder, GuestOptionsBuilder};
use std::{collections::HashMap, env, fs, path::PathBuf};

// RISC Zero's guest builder for Rust 1.97, pinned by content digest.
const BUILDER_IMAGE: &str =
    "r0.1.97.0@sha256:7ae0a27fa72b10fb5c1066005bd949ae43dc4ead8a284f2d6572b99c82ab4468";

fn main() -> Result<()> {
    // The nested guest build uses the RISC Zero compiler, not host Clippy's driver.
    #[expect(
        unsafe_code,
        reason = "the build script changes its environment before starting any threads"
    )]
    // SAFETY: This process is single-threaded here, before calling RISC Zero.
    unsafe {
        env::remove_var("RUSTC_WORKSPACE_WRAPPER");
    }
    if env::var_os("CARGO_FEATURE_REPRODUCIBLE").is_none() {
        risc0_build::embed_methods();
        return Ok(());
    }
    #[expect(
        unsafe_code,
        reason = "the build script changes its environment before starting any threads"
    )]
    // SAFETY: This process is single-threaded here, before calling RISC Zero.
    // The image has only an amd64 build; other hosts must run it in emulation.
    unsafe {
        env::set_var("DOCKER_DEFAULT_PLATFORM", "linux/amd64");
    }
    reproducible()
}

/// Build the guest in the pinned Docker image and require the committed image ID.
fn reproducible() -> Result<()> {
    for name in ["RISC0_DOCKER_CONTAINER_TAG", "RISC0_SKIP_BUILD"] {
        println!("cargo:rerun-if-env-changed={name}");
        if env::var_os(name).is_some() {
            bail!("{name} must be unset for a reproducible build");
        }
    }
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let root = manifest_dir.parent().context("workspace root not found")?;
    let docker = DockerOptionsBuilder::default()
        .root_dir(root)
        .docker_container_tag(BUILDER_IMAGE)
        .build()?;
    let options = GuestOptionsBuilder::default().use_docker(docker).build()?;
    let methods =
        risc0_build::embed_methods_with_options(HashMap::from([("greenhold-guest", options)]));
    let [method] = methods.as_slice() else {
        bail!("expected exactly one guest program");
    };

    let path = manifest_dir.join("image-id");
    println!("cargo:rerun-if-changed={}", path.display());
    let expected = fs::read_to_string(&path).context("cannot read guest-builder/image-id")?;
    let actual = method.image_id.to_string();
    if actual != expected.trim() {
        bail!("guest image ID {actual} does not match guest-builder/image-id");
    }
    Ok(())
}
