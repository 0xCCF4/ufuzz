#![crate_type = "proc-macro"]
extern crate proc_macro;
use proc_macro::TokenStream;
use std::path::PathBuf;
use ucode_compiler_bridge::CompilerOptions;
use unindent::unindent;

/// This macro must be used at module level. It will create a module containing the patch content.
///
/// # Example
///
/// ```text
/// mod test_patch {
///     patch!(
///         SOME ASSEMBLY CODE
///     )
/// }
#[proc_macro]
pub fn patch(_item: TokenStream) -> TokenStream {
    // todo: change syntax to patch! { ... }

    let text = proc_macro::Span::call_site()
        .source_text()
        .expect("Failed to get source text from patch! macro!");
    if text.len() < 8 {
        unreachable!("This should not have happened! Source text should be at least panic!()");
    }
    let text = &text[7..text.len() - 1];
    let text = unindent(text);

    let result = compile(text.as_str());

    result.parse().unwrap()
}

fn compile(text: &str) -> String {
    let project_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("Failed to get project dir for patch! macro!"),
    );

    let file = tempfile::tempdir().expect("Failed to create temporary dir for patch! macro!");

    let original_dir = file.path().join("original");
    let processed_dir = file.path().join("processed");
    let compiled_dir = file.path().join("compiled");

    std::fs::create_dir(&original_dir).expect("Failed to create original dir for patch! macro!");
    std::fs::create_dir(&processed_dir).expect("Failed to create processed dir for patch! macro!");
    std::fs::create_dir(&compiled_dir).expect("Failed to create compiled dir for patch! macro!");

    let source_path = original_dir.join("patch.u");
    let dest_path = compiled_dir.join("patch.rs");

    std::fs::write(&source_path, text).expect("Failed to write patch! macro source");

    ucode_compiler_bridge::preprocess_scripts(
        &original_dir,
        &processed_dir,
        project_dir.join("patches"),
    )
    .unwrap_or_else(|err| panic!("Failed to preprocess patch! macro: {}", err));
    ucode_compiler_bridge::build_script_compile_folder(
        &processed_dir,
        &compiled_dir,
        CompilerOptions {
            allow_unused: true,
            avoid_unknown_256: true,
            cpuid: None,
        },
    )
    .unwrap_or_else(|err| panic!("Failed to compile patch! macro: {}", err));

    std::fs::read_to_string(&dest_path).expect("Failed to read compiled patch! macro!")
}
