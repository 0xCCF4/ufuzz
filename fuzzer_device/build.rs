use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let git_hash = Command::new("git")
        .args(&["rev-parse", "HEAD"])
        .output()
        .map(|output| String::from_utf8(output.stdout).unwrap())
        .expect("Failed to get git hash");
    let hash = chrono::Utc::now().to_rfc2822();

    let mut hasher = Sha256::new();
    hasher.update(hash.as_bytes());
    hasher.update(git_hash.as_bytes());

    let hash = format!("{:x}", hasher.finalize());
    const NUMBER_OF_BYTES: usize = 4;

    let hash = &hash[..NUMBER_OF_BYTES * 2];

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-env=BUILD_TIMESTAMP_HASH={}", hash);

    // recursive for each src file

    let mut queue = Vec::new();
    queue.push(PathBuf::from("src"));

    while let Some(file) = queue.pop() {
        println!("cargo:rerun-if-changed={}", file.to_string_lossy());
        if file.is_dir() {
            for entry in std::fs::read_dir(file).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if path.is_dir() {
                    queue.push(path);
                } else {
                    println!("cargo:rerun-if-changed={}", path.to_string_lossy());
                }
            }
        }
    }
}
