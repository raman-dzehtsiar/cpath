// SPDX-License-Identifier: MIT
fn main() {
    let mut build = cc::Build::new();
    build.std("c11");
    for dialect in ["typescript", "tsx"] {
        let directory = format!("{dialect}/src");
        build.include(&directory);
        for suffix in [
            "",
            "_external_scanner_create",
            "_external_scanner_destroy",
            "_external_scanner_serialize",
            "_external_scanner_deserialize",
            "_external_scanner_scan",
        ] {
            let original = format!("tree_sitter_{dialect}{suffix}");
            let renamed = format!("cpath_{original}");
            build.define(&original, Some(renamed.as_str()));
        }
        for file in ["parser.c", "scanner.c"] {
            let path = format!("{directory}/{file}");
            build.file(&path);
            println!("cargo:rerun-if-changed={path}");
        }
        println!("cargo:rerun-if-changed={directory}/tree_sitter");
    }
    println!("cargo:rerun-if-changed=common/scanner.h");
    build.compile("tree-sitter-typescript-cpath");
}
