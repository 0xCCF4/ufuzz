use sha2::{Digest, Sha256};
use std::process::Command;

fn main() {
    let output = Command::new("git")
        .args(&["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let git_hash = String::from_utf8(output.stdout).unwrap();
    let hash = chrono::Utc::now().to_rfc2822();

    let mut hasher = Sha256::new();
    hasher.update(hash.as_bytes());
    hasher.update(git_hash.as_bytes());

    let hash = format!("{:x}", hasher.finalize());
    const NUMBER_OF_BYTES: usize = 4;

    let hash = &hash[..NUMBER_OF_BYTES * 2];

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-env=BUILD_TIMESTAMP_HASH={}", hash);
}
