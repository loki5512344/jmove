//! Tree-sitter based frontend for TypeScript/JavaScript.
//!
//! CONTRACT: see [`crate::parser`]. Implement `extract_imports` using
//! `tree-sitter-typescript` grammars.

use super::{ImportRecord, Language, SourceLanguage};

/// Frontend backed by the tree-sitter TypeScript/TSX/JS grammar.
pub struct TreeSitterTs {
    lang: SourceLanguage,
}

impl TreeSitterTs {
    /// Create a frontend for the given language variant.
    #[must_use]
    pub fn new(lang: SourceLanguage) -> Self {
        Self { lang }
    }
}

impl Language for TreeSitterTs {
    fn language(&self) -> SourceLanguage {
        self.lang
    }

    fn extract_imports(&self, _source: &str) -> Vec<ImportRecord> {
        todo!("parser agent: implement tree-sitter extraction")
    }
}
