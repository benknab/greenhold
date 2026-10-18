fn main() {
    // The nested guest build uses the RISC Zero compiler, not host Clippy's driver.
    #[expect(
        unsafe_code,
        reason = "the build script changes its environment before starting any threads"
    )]
    // SAFETY: This process is single-threaded here, before calling RISC Zero.
    unsafe {
        std::env::remove_var("RUSTC_WORKSPACE_WRAPPER");
    }
    risc0_build::embed_methods();
}
