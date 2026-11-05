// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

use std::path::PathBuf;

pub mod cognitive;

/// Grammar selection is owned by the file-selection policy. Shared C/C++
/// headers try C first, then C++ only if the C grammar rejects the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceLanguage {
    CSharp,
    C,
    Cpp,
    CHeader,
    Rust,
    Java,
    Python,
    JavaScript,
    TypeScript,
    Tsx,
    Go,
}

/// Parser-independent facts owned by one analysis worker; never retained across files.
#[derive(Debug, Clone, Default)]
pub struct FileFacts {
    pub path: PathBuf,
    pub bytes: u64,
    pub lines: u32,
    pub methods: Vec<MethodFacts>,
    /// Optional C# scope/edge facts for the Sonar-compatible cognitive profile.
    /// The legacy independent-callable audit uses `methods`.
    pub cognitive: Option<cognitive::CognitiveFacts>,
    /// Other languages retain separately typed cognitive profiles.
    pub profiled_cognitive: Option<crate::metrics::CognitiveInput>,
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
    /// Nonblank physical lines containing code; None when LOC was not requested.
    pub lines_of_code: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct ComplexityMetrics {
    /// Eligible units remain available, but unsupported source scopes prevent
    /// treating their aggregate as a complete file score.
    pub has_unscored_scopes: bool,
    /// Maximum rankable unit, including supported module/initializer scopes.
    /// The field name is retained for compatibility with the original API.
    pub max_method: u32,
    /// The profile's file aggregate; do not reconstruct it by summing units.
    pub total: u32,
    /// Rankable source units; some profiles also expose non-callable scopes.
    pub methods: Vec<MethodComplexity>,
}

#[derive(Debug, Clone)]
pub struct MethodComplexity {
    pub name: String,
    /// Exact source interval when provided by the language profile.
    pub span: Option<cognitive::Span>,
    pub start_line: u32,
    pub end_line: u32,
    pub score: u32,
}

/// Report evidence only. Unlike ComplexityMetrics, methods is a bounded unit sample
/// and must not be used to recompute a whole-file score.
#[derive(Debug, Clone, Default)]
pub struct ComplexitySummary {
    pub max_method: u32,
    pub total: u32,
    pub methods: Vec<MethodComplexity>,
}
