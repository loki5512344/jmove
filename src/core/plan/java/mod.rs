//! Java move planning: the package/directory-coupled flavour of
//! [`super::plan_move`].
//!
//! A Java move is three coordinated edits (see `docs/PLAN.md`):
//! 1. the moved file's `package` declaration,
//! 2. every `import <fqn>` that resolves to the moved class,
//! 3. the physical move itself (apply layer).
//!
//! The new package is derived through the *source root* — the directory the
//! old package is relative to (`src/main/java`, `src`, ...). Importers speak
//! absolute FQNs, so unlike TS no per-importer relative math is needed.

use std::path::{Path, PathBuf};

use crate::core::index::Index;
use crate::core::plan::Rewrite;
use crate::core::{JmoveError, JmoveResult};

/// Rewrite set for moving `source.java` to `target.java` (both
/// project-relative, validated by the caller).
pub(super) fn java_rewrites(
    index: &Index,
    source: &Path,
    target: &Path,
) -> JmoveResult<Vec<Rewrite>> {
    let rejected = |what: String| JmoveError::PlanRejected(format!("Java move: {what}"));

    if target.extension().is_none_or(|e| e != "java") {
        return Err(rejected(format!(
            "'{}' is a .java file, the target must keep the .java extension",
            source.display()
        )));
    }
    let sdir = source.parent().unwrap_or(Path::new(""));
    let tdir = target.parent().unwrap_or(Path::new(""));
    let Some(decl) = index.packages.get(source) else {
        // Default package: un-importable, so only an in-place rename is safe.
        return if sdir == tdir {
            Ok(Vec::new())
        } else {
            Err(rejected(format!(
                "'{}' has no `package` declaration (default package); it can only be renamed inside its directory",
                source.display()
            )))
        };
    };

    let pkg = decl.name.as_str();
    let pkg_path = pkg.replace('.', "/");
    if !sdir.ends_with(Path::new(&pkg_path)) {
        return Err(rejected(format!(
            "package '{pkg}' does not match directory '{}'",
            sdir.display()
        )));
    }
    let src_root = strip_package_dir(sdir, pkg);
    let rest = tdir.strip_prefix(&src_root).map_err(|_| {
        rejected(format!(
            "target directory '{}' is outside the Java source root '{}'",
            tdir.display(),
            src_root.display()
        ))
    })?;
    let new_pkg = package_of(rest);
    if new_pkg.is_empty() {
        return Err(rejected(
            "the target directory maps to the default package; importers could not reference the class"
                .to_owned(),
        ));
    }

    let stem_old = file_stem(source)?;
    let stem_new = file_stem(target)?;
    let (old_fqn, new_fqn) = (format!("{pkg}.{stem_old}"), format!("{new_pkg}.{stem_new}"));
    let mut rewrites = Vec::new();

    // 1. the moved file's own package declaration.
    if new_pkg != pkg {
        rewrites.push(Rewrite {
            file: source.to_path_buf(),
            span: decl.span.clone(),
            old_text: pkg.to_owned(),
            new_text: new_pkg,
        });
    }
    // 2. every importer edge that resolves to the moved class.
    for importer in index.importers_of(source) {
        for edge in index.imports[&importer]
            .iter()
            .filter(|e| e.target.as_deref() == Some(source))
        {
            let new_text = rewrite_fqn(&old_fqn, &new_fqn, &edge.record.specifier);
            if new_text == edge.record.specifier {
                continue; // no-op rewrite, never reaches the plan
            }
            rewrites.push(Rewrite {
                file: importer.clone(),
                span: edge.record.span.clone(),
                old_text: edge.record.specifier.clone(),
                new_text,
            });
        }
    }
    rewrites.sort_by_key(|r| (r.file.clone(), r.span.start));
    Ok(rewrites)
}

// Directory `sdir` minus the trailing components of package `pkg`.
fn strip_package_dir(sdir: &Path, pkg: &str) -> PathBuf {
    let keep = sdir.components().count() - pkg.split('.').count();
    sdir.components().take(keep).collect()
}

// `"com/example"` -> `"com.example"` (empty dir -> empty/default package).
fn package_of(dir: &Path) -> String {
    dir.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join(".")
}

fn file_stem(path: &Path) -> JmoveResult<String> {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_owned)
        .ok_or_else(|| {
            JmoveError::InvalidArgument(format!("invalid file name '{}'", path.display()))
        })
}

// Swap the class-FQN prefix inside an import specifier; member imports
// (`com.example.Parser.parse` from `import static`) keep their member part.
fn rewrite_fqn(old_fqn: &str, new_fqn: &str, specifier: &str) -> String {
    match specifier.strip_prefix(old_fqn) {
        Some(rest) if rest.starts_with('.') => format!("{new_fqn}{rest}"),
        _ => new_fqn.to_owned(),
    }
}

#[cfg(test)]
mod tests;
