//! Project indexing: gitignore-aware filesystem scan plus the import graph.
//!
//! Built fresh on every command (disk cache is Phase 2). `ignore::WalkBuilder`
//! handles `.gitignore`/hidden-file rules; every indexed TS/JS source file is
//! parsed through [`crate::parser`] and its specifiers resolved through
//! [`crate::parser::resolve`].

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

use crate::core::{JmoveResult, normalize_rel_path};
use crate::parser::resolve::resolve_module;
use crate::parser::{ImportRecord, Language, SourceLanguage, frontend_for};

/// Indexed source files with O(1) membership lookups.
#[derive(Debug, Default)]
pub struct FileSet {
    paths: HashSet<PathBuf>,
}

impl FileSet {
    /// Add a normalized project-relative path; `false` if already present.
    pub fn add(&mut self, path: PathBuf) -> bool {
        self.paths.insert(path)
    }

    /// Whether `path` is a known indexed source file.
    #[must_use]
    pub fn contains(&self, path: &Path) -> bool {
        self.paths.contains(path)
    }

    /// All files in deterministic sorted order (stable for tests and diffs).
    #[must_use]
    pub fn sorted(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = self.paths.iter().cloned().collect();
        v.sort();
        v
    }
}

/// One import occurrence plus the project file it resolves to.
/// `target: None` means "external" — a bare package specifier or a path
/// that does not exist in the index.
#[derive(Debug, Clone)]
pub struct ResolvedImport {
    /// Raw record from the parser (specifier text + byte span).
    pub record: ImportRecord,
    /// Project-relative resolved file, if any.
    pub target: Option<PathBuf>,
}

/// Full in-memory project index: file set and forward import edges.
#[derive(Debug, Default)]
pub struct Index {
    /// Absolute project root the index was built for.
    pub root: PathBuf,
    /// All indexed source files.
    pub files: FileSet,
    /// For each file, the imports it declares (in source order).
    pub imports: HashMap<PathBuf, Vec<ResolvedImport>>,
}

impl Index {
    /// Scan `root`, parse every supported source file and build the graph.
    /// Unreadable or unparseable files are skipped, not fatal.
    pub fn build(root: &Path) -> JmoveResult<Self> {
        let root = root.canonicalize()?;
        let mut index = Self {
            root,
            files: FileSet::default(),
            imports: HashMap::new(),
        };
        index.scan()?;
        // Resolution needs the complete file set (extension/index guessing),
        // so it runs as a second pass over the staged records.
        for (importer, imports) in &mut index.imports {
            for resolved in imports {
                resolved.target =
                    resolve_module(importer, &resolved.record.specifier, &index.files);
            }
        }
        Ok(index)
    }

    // Walk the project and parse each supported source file, staging the
    // raw records with `target: None` for the resolution pass above.
    fn scan(&mut self) -> JmoveResult<()> {
        // Sorted map: deterministic discovery order.
        let mut found: BTreeMap<PathBuf, SourceLanguage> = BTreeMap::new();
        for entry in WalkBuilder::new(&self.root).require_git(false).build() {
            // Walker errors (unreadable dirs, etc.) simply skip the entry.
            let Ok(entry) = entry else { continue };
            if entry.path_is_symlink() || !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            let stripped = entry
                .path()
                .strip_prefix(&self.root)
                .unwrap_or(Path::new(""));
            let Some(rel) = normalize_rel_path(stripped) else {
                continue;
            };
            if let Some(lang) = SourceLanguage::for_path(&rel) {
                found.insert(rel, lang);
            }
        }
        // One frontend per language, created lazily as languages appear.
        let mut frontends: Vec<(SourceLanguage, Box<dyn Language>)> = Vec::new();
        for (rel, lang) in found {
            let Ok(text) = fs::read_to_string(self.root.join(&rel)) else {
                continue; // non-UTF-8 or vanished between scan and read
            };
            if !frontends.iter().any(|(l, _)| *l == lang) {
                frontends.push((lang, frontend_for(lang)));
            }
            let frontend = &mut frontends
                .iter_mut()
                .find(|(l, _)| *l == lang)
                .expect("frontend was just ensured")
                .1;
            let records = frontend.extract_imports(&text);
            self.files.add(rel.clone());
            self.imports.insert(
                rel,
                records
                    .into_iter()
                    .map(|record| ResolvedImport {
                        record,
                        target: None,
                    })
                    .collect(),
            );
        }
        Ok(())
    }

    /// Reverse edge lookup: every indexed file that imports `target`.
    #[must_use]
    pub fn importers_of(&self, target: &Path) -> Vec<PathBuf> {
        let mut importers: Vec<PathBuf> = self
            .imports
            .iter()
            .filter(|(_, imports)| imports.iter().any(|r| r.target.as_deref() == Some(target)))
            .map(|(file, _)| file.clone())
            .collect();
        importers.sort();
        importers
    }
}

#[cfg(test)]
mod tests {
    use super::{Index, ResolvedImport};
    use crate::core::JmoveResult;
    use crate::parser::ImportRecord;
    use std::fs;
    use std::path::{Path, PathBuf};

    // Write `rel` (creating parent dirs) inside `root`.
    fn write_file(root: &Path, rel: &str, body: &str) -> JmoveResult<()> {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(path, body)?;
        Ok(())
    }

    // Stub edge with resolved target `target` (parser-independent).
    fn edge(target: &str) -> ResolvedImport {
        let record = ImportRecord {
            specifier: format!("./{target}"),
            span: 0..0,
            is_dynamic: false,
        };
        ResolvedImport {
            record,
            target: Some(PathBuf::from(target)),
        }
    }

    #[test]
    fn build_skips_ignored_and_unsupported_files() -> JmoveResult<()> {
        let dir = tempfile::TempDir::new()?;
        let root = dir.path();
        fs::create_dir(root.join(".git"))?;
        write_file(root, ".gitignore", "ignored/\n")?;
        write_file(root, "src/a.ts", "import { b } from './b';\n")?;
        write_file(root, "src/b.ts", "export const b = 1;\n")?;
        write_file(root, "src/legacy.js", "const a = require('./a');\n")?;
        write_file(root, "ignored/c.ts", "export const c = 1;\n")?;
        write_file(root, "docs/note.md", "not source\n")?;
        fs::write(root.join("src/binary.ts"), [0xff_u8, 0xfe, 0x00, 0x01])?;

        let index = Index::build(root)?;
        assert!(index.files.contains(Path::new("src/a.ts")));
        assert!(index.files.contains(Path::new("src/b.ts")));
        assert!(index.files.contains(Path::new("src/legacy.js")));
        // gitignored, non-source and non-UTF-8 files must never be indexed.
        for skipped in ["ignored/c.ts", "docs/note.md", "src/binary.ts"] {
            assert!(!index.files.contains(Path::new(skipped)), "{skipped}");
        }
        Ok(())
    }

    #[test]
    fn build_resolves_relative_specifier_to_indexed_file() -> JmoveResult<()> {
        let dir = tempfile::TempDir::new()?;
        let root = dir.path();
        write_file(root, "src/a.ts", "import { b } from './b';\n")?;
        write_file(root, "src/b.ts", "export const b = 1;\n")?;

        let index = Index::build(root)?;
        let imports = index
            .imports
            .get(Path::new("src/a.ts"))
            .expect("a.ts must be indexed");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].record.specifier, "./b");
        assert_eq!(imports[0].target.as_deref(), Some(Path::new("src/b.ts")));
        assert_eq!(index.root, fs::canonicalize(root)?);
        Ok(())
    }

    #[test]
    fn importers_of_returns_sorted_reverse_edges() -> JmoveResult<()> {
        let dir = tempfile::TempDir::new()?;
        let mut index = Index::build(dir.path())?;
        // Stub the graph so reverse-edge logic is independent of the parser.
        index.imports.clear();
        let edges = [
            ("z.ts", "shared.ts"),
            ("a.ts", "shared.ts"),
            ("m.ts", "other.ts"),
        ];
        for (file, target) in edges {
            index
                .imports
                .insert(PathBuf::from(file), vec![edge(target)]);
        }
        assert_eq!(
            index.importers_of(Path::new("shared.ts")),
            vec![PathBuf::from("a.ts"), PathBuf::from("z.ts")]
        );
        assert!(index.importers_of(Path::new("missing.ts")).is_empty());
        Ok(())
    }
}
