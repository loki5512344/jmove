//! Language frontends: import extraction and module specifier resolution.
//!
//! ## Contract (stable across submodules — implementers must not change it)
//!
//! - A [`Language`] parses one source file into [`ImportRecord`]s: every
//!   *static-ish* module reference (TS `import`/`export from`/`require`/
//!   dynamic `import()`; Java single-type and static-member `import`s —
//!   on-demand `pkg.*` imports are deliberately not extracted).
//! - Java files additionally expose their [`PackageDecl`] via
//!   [`Language::extract_package`]; resolution of Java specifiers goes
//!   through [`java::JavaClassIndex`] instead of [`resolve`] (TS relative
//!   specifiers). External Java imports (jdk, third-party) simply never
//!   appear in the class index.
//! - Rewrites must touch **only the specifier string**, never the rest of
//!   the statement (KISS + no formatter dependency): that is why
//!   [`ImportRecord::span`] is a byte range into the original source.
//! - Fix rules (Phase 1.6) implement [`Fix`] and propose [`FixCandidate`]s:
//!   byte [`Edit`](crate::core::Edit)s for one file, run through the same
//!   dry-run/atomic engine as `mv`. Deterministic by contract — ambiguity
//!   is reported as `auto_fixable: false` for an agent to resolve, never
//!   guessed at.

use std::ops::Range;
use std::path::Path;

use crate::core::Edit;
use crate::core::index::Index;

pub mod java;
pub mod resolve;
pub mod ts;

/// Source languages `jmove` understands (TS/JS in Phase 1, Java in Phase 1.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceLanguage {
    /// `.ts` (non-TSX) sources.
    TypeScript,
    /// `.tsx` / `.jsx` sources.
    Tsx,
    /// Plain `.js` / `.mjs` / `.cjs` sources.
    JavaScript,
    /// `.java` sources.
    Java,
}

impl SourceLanguage {
    /// Map a file extension (lowercase, no dot) to a language, if supported.
    #[must_use]
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "ts" | "mts" | "cts" => Some(Self::TypeScript),
            "tsx" | "jsx" => Some(Self::Tsx),
            "js" | "mjs" | "cjs" => Some(Self::JavaScript),
            "java" => Some(Self::Java),
            _ => None,
        }
    }

    /// Detect the language from a file name. `None` means "not a source file
    /// we index" (skip it).
    #[must_use]
    pub fn for_path(path: &Path) -> Option<Self> {
        Self::from_extension(
            path.extension()
                .and_then(|e| e.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref()?,
        )
    }
}

/// One module reference found in a source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportRecord {
    /// Raw specifier text as written, e.g. `"../utils/format"` (TS) or
    /// `"com.example.utils.Parser"` (Java).
    pub specifier: String,
    /// Byte range of the *specifier string contents* (inside the quotes,
    /// without the quote characters) in the parsed file. The rewriter
    /// replaces exactly this span and nothing else.
    pub span: Range<usize>,
    /// `true` for dynamic `import("...")` / `require("...")` occurrences.
    pub is_dynamic: bool,
}

/// A Java `package` declaration: dotted name plus the byte span of the name
/// (quotes have no meaning here — the span covers `com.example.utils`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageDecl {
    /// Declared package name, e.g. `"com.example.utils"`.
    pub name: String,
    /// Byte range of the package name in the parsed file.
    pub span: Range<usize>,
}

/// A language frontend that extracts imports from source text.
pub trait Language: Send + Sync {
    /// The language this frontend handles.
    fn language(&self) -> SourceLanguage;

    /// Extract all import records from `source` in byte-offset order.
    /// Parse errors must not be fatal: return what was understood.
    fn extract_imports(&self, source: &str) -> Vec<ImportRecord>;

    /// Extract the `package` declaration, if this language has one and the
    /// file declares it. Default: no package concept (TS/JS).
    fn extract_package(&self, _source: &str) -> Option<PackageDecl> {
        None
    }
}

/// Build the default frontend for `lang`.
#[must_use]
pub fn frontend_for(lang: SourceLanguage) -> Box<dyn Language> {
    match lang {
        SourceLanguage::Java => Box::new(java::TreeSitterJava::new()),
        SourceLanguage::TypeScript | SourceLanguage::Tsx | SourceLanguage::JavaScript => {
            Box::new(ts::TreeSitterTs::new(lang))
        }
    }
}

/// Urgency of a [`FixCandidate`] for the reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The code does not compile (or resolves) without the fix.
    Error,
    /// The code works but is dirty: unused or misordered imports.
    Warning,
    /// Style note only.
    Info,
}

impl Severity {
    /// Stable lowercase name used in `--json` output.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }

    /// Overlap-resolution priority: on a byte conflict the more urgent
    /// rule wins and the other candidate is deferred to the next run.
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Self::Error => 0,
            Self::Warning => 1,
            Self::Info => 2,
        }
    }
}

/// One proposed fix for a single file: what is wrong and — when
/// `auto_fixable` — the exact edits that repair it.
#[derive(Debug, Clone)]
pub struct FixCandidate {
    /// Stable rule id; also the value accepted by `jmove fix --rule`.
    pub rule: &'static str,
    /// One-line explanation for humans and agents.
    pub message: String,
    /// How urgent the fix is.
    pub severity: Severity,
    /// `false` marks an ambiguous finding an agent or human must resolve;
    /// the engine never applies such candidates automatically.
    pub auto_fixable: bool,
    /// Byte range of the offending construct (issue location for reports,
    /// even when `edits` is empty).
    pub span: Range<usize>,
    /// Edits on this file, ascending by span.
    pub edits: Vec<Edit>,
    /// Fully-qualified options for an ambiguous finding (`auto_fixable:
    /// false`); empty for every auto-fixable or non-lookup candidate.
    pub candidates: Vec<String>,
}

/// A deterministic single-file fix rule (Phase 1.6).
pub trait Fix: Send + Sync {
    /// Stable rule id reported in candidates and accepted by `--rule`.
    fn rule(&self) -> &'static str;

    /// Candidates for `path` (project-relative) with the given contents.
    /// A rule may only return `auto_fixable: true` edits it can prove
    /// safe; anything ambiguous goes out as a non-auto candidate.
    fn fixes(&self, path: &Path, source: &str, index: &Index) -> Vec<FixCandidate>;
}

/// Default rule set for `lang` (empty for languages without rules yet).
#[must_use]
pub fn fixers_for(lang: SourceLanguage) -> Vec<Box<dyn Fix>> {
    match lang {
        SourceLanguage::Java => vec![
            Box::new(java::rules::unused_imports::JavaUnusedImports::new()),
            Box::new(java::rules::missing_imports::JavaMissingImports::new()),
            Box::new(java::rules::import_order::JavaImportOrder::new()),
        ],
        SourceLanguage::TypeScript | SourceLanguage::Tsx | SourceLanguage::JavaScript => Vec::new(),
    }
}

/// Every rule id `jmove fix` currently knows about.
#[must_use]
pub fn rule_ids() -> &'static [&'static str] {
    &[
        java::rules::unused_imports::RULE,
        java::rules::missing_imports::RULE,
        java::rules::import_order::RULE,
    ]
}
