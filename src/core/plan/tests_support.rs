//! Shared `#[cfg(test)]` graph builders for the plan submodules.

use std::ops::Range;
use std::path::PathBuf;

use crate::core::index::ResolvedImport;
use crate::parser::ImportRecord;

/// Hand-wired resolved edge: plan tests never touch the parser.
pub(crate) fn edge(spec: &str, span: Range<usize>, target: &str) -> ResolvedImport {
    let record = ImportRecord {
        specifier: spec.into(),
        span,
        is_dynamic: false,
    };
    ResolvedImport {
        record,
        target: Some(PathBuf::from(target)),
    }
}
