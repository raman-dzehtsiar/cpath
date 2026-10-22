// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

use std::path::PathBuf;

/// Grammar selection is owned by the file-selection policy. Shared C/C++
/// headers try C first, then C++ only if the C grammar rejects the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceLanguage {
    CSharp,
    C,
    Cpp,
    CHeader,
}

/// Parser-independent facts owned by one analysis worker; never retained across files.
#[derive(Debug, Clone, Default)]
pub struct FileFacts {
    pub path: PathBuf,
    pub bytes: u64,
    pub lines: u32,
    pub methods: Vec<MethodFacts>,
}

#[derive(Debug, Clone)]
pub struct MethodFacts {
    pub name: String,
    pub start_line: u32,
    pub end_line: u32,
    pub events: Vec<ComplexityEvent>,
}

#[derive(Debug, Clone)]
pub struct ComplexityEvent {
    pub kind: ControlKind,
    pub nesting: u32,
    pub line: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlKind {
    If,
    Else,
    Loop,
    Switch,
    Catch,
    Ternary,
    BooleanSequence,
    Jump,
}

/// Compact per-file metadata retained after worker-local syntax facts are dropped.
#[derive(Debug, Clone)]
pub struct FileSummary {
    pub path: PathBuf,
    pub bytes: u64,
    pub lines: u32,
}

#[derive(Debug, Clone, Default)]
pub struct ComplexityMetrics {
    pub max_method: u32,
    pub total: u32,
    pub methods: Vec<MethodComplexity>,
}

#[derive(Debug, Clone)]
pub struct MethodComplexity {
    pub name: String,
    pub start_line: u32,
    pub end_line: u32,
    pub score: u32,
}

/// Report evidence only. Unlike ComplexityMetrics, methods is a bounded sample
/// and must not be used to recompute a whole-file score.
#[derive(Debug, Clone, Default)]
pub struct ComplexitySummary {
    pub max_method: u32,
    pub total: u32,
    pub methods: Vec<MethodComplexity>,
}
