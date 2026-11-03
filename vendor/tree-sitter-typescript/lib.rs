// SPDX-License-Identifier: MIT
//! Corrected cognitive grammar; the original package remains the RCA MI grammar.
use tree_sitter_language::LanguageFn;

extern "C" {
    fn cpath_tree_sitter_typescript() -> *const ();
    fn cpath_tree_sitter_tsx() -> *const ();
}

pub const LANGUAGE_TYPESCRIPT: LanguageFn =
    unsafe { LanguageFn::from_raw(cpath_tree_sitter_typescript) };
pub const LANGUAGE_TSX: LanguageFn = unsafe { LanguageFn::from_raw(cpath_tree_sitter_tsx) };
