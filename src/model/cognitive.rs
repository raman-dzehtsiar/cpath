// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

//! Syntax facts for scoped cognitive complexity, without parser-owned nodes.
use super::ControlKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    File,
    Method,
    Lambda,
    Local { is_static: bool },
    TopLevel,
    Field,
    Other,
}

#[derive(Debug, Clone)]
pub struct Signature {
    pub name: String,
    pub arguments: usize,
}

#[derive(Debug, Clone)]
pub enum Event {
    Control {
        kind: ControlKind,
        nesting: u32,
    },
    Logical {
        span: Span,
        same_left: bool,
        same_right: Option<Span>,
    },
    Invocation(Signature),
    Child {
        index: usize,
        nesting: u32,
    },
}

#[derive(Debug, Clone)]
pub struct Scope {
    pub kind: ScopeKind,
    pub syntax_kind: String,
    pub name: String,
    pub span: Span,
    pub start_line: u32,
    pub end_line: u32,
    pub signature: Option<Signature>,
    pub events: Vec<Event>,
}

#[derive(Debug, Clone)]
pub struct CognitiveFacts {
    /// Preorder scope graph; scope zero is the whole file. Child edges retain
    /// lexical order and entry nesting, including fields and nested callables.
    pub scopes: Vec<Scope>,
    pub top_level: Option<usize>,
}
