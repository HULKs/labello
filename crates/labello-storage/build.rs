fn main() {
    let lock = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("../../Cargo.lock");
    println!("cargo:rerun-if-changed={}", lock.display());
    let bytes =
        std::fs::read(lock).expect("preview encoder identity requires the workspace lockfile");
    println!(
        "cargo:rustc-env=LABELLO_PREVIEW_DEPENDENCIES={}",
        blake3::hash(&bytes)
    );
}
