// SPDX-License-Identifier: MIT
//! Compatibility language derived from the pinned sibling C++ grammar.

use tree_sitter_language::LanguageFn;

extern "C" {
    fn tree_sitter_cpp_compat() -> *const ();
}

pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_cpp_compat) };
