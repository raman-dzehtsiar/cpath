fn main() {
    let src_dir = std::path::Path::new("src");

    let mut c_config = cc::Build::new();
    c_config.std("c11").include(src_dir);

    #[cfg(target_env = "msvc")]
    c_config.flag("-utf-8");

    let parser_path = src_dir.join("parser.c");
    c_config.file(&parser_path);
    println!("cargo:rerun-if-changed={}", parser_path.to_str().unwrap());

    c_config.compile("tree-sitter-java");

    let reference_dir = src_dir.join("rca");
    let reference_path = reference_dir.join("parser.c");
    let reference_header = reference_dir.join("tree_sitter/parser.h");
    let mut reference = cc::Build::new();
    reference.std("c11").include(&reference_dir);
    #[cfg(target_env = "msvc")]
    reference.flag("-utf-8");
    reference.define("tree_sitter_java", Some("tree_sitter_java_rca"));
    reference.file(&reference_path);
    println!(
        "cargo:rerun-if-changed={}",
        reference_path.to_str().unwrap()
    );
    println!(
        "cargo:rerun-if-changed={}",
        reference_header.to_str().unwrap()
    );
    reference.compile("tree-sitter-java-rca");
}
