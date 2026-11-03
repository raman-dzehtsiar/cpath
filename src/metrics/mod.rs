// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

//! Explicit language profiles. No profile inherits another language's formula.

pub mod jvm_python;
pub mod native;
pub mod web_go;

use crate::maintainability::MaintainabilityMetrics;
use crate::model::ComplexityMetrics;

#[derive(Debug, Clone)]
pub enum CognitiveInput {
    Native(native::CognitiveFacts),
    JvmPython(jvm_python::CognitiveFacts),
    WebGo(web_go::CognitiveFacts),
}

#[derive(Debug, Clone, PartialEq)]
pub enum MaintainabilityInput {
    Native(native::MaintainabilityFacts),
    JvmPython(jvm_python::MaintainabilityFacts),
    WebGo(web_go::MaintainabilityFacts),
}

pub fn analyze_cognitive(facts: &CognitiveInput) -> ComplexityMetrics {
    match facts {
        CognitiveInput::Native(facts) => native::analyze_cognitive(facts),
        CognitiveInput::JvmPython(facts) => jvm_python::analyze_cognitive(facts),
        CognitiveInput::WebGo(facts) => web_go::analyze_cognitive(facts),
    }
}

pub fn analyze_maintainability(facts: &MaintainabilityInput) -> MaintainabilityMetrics {
    match facts {
        MaintainabilityInput::Native(facts) => native::analyze_maintainability(facts),
        MaintainabilityInput::JvmPython(facts) => jvm_python::analyze_maintainability(facts),
        MaintainabilityInput::WebGo(facts) => web_go::analyze_maintainability(facts),
    }
}
