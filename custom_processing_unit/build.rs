use std::fs;
use std::path::PathBuf;

fn main() {
    build_patches();
}

fn build_patches() {
    let target = PathBuf::from("src/patches");

    if !target.exists() {
        fs::create_dir(&target).expect("Failed to create patches directory");
    } else {
        for file in target.read_dir().expect("Directory not readable") {
            let file = file.expect("File not found").path();
            if file
                .extension()
                .map(|v| v.to_string_lossy().to_string())
                .unwrap_or("".to_string())
                .as_str()
                == "rs"
            {
                fs::remove_file(file).expect("Unable to remove file");
            }
        }
    }

    let names = ucode_compiler::build_script(&PathBuf::from("patches"), &target);

    let content = names.iter().fold(String::new(), |acc, name| {
        acc + &format!("mod {name};\npub use {name}::PATCH as {name};\n")
    });
    std::fs::write("src/patches.rs", content).expect("Failed to write patches.rs");
}
