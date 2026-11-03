// SPDX-License-Identifier: MIT
fn main() {
    cc::Build::new()
        .std("c11")
        .include("src")
        .file("src/parser.c")
        .file("src/scanner.c")
        .compile("tree-sitter-cpp-compat");
    for path in [
        "src/parser.c",
        "src/scanner.c",
        "../tree-sitter-cpp/src/scanner.c",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
}
