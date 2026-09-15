# jmove skill (for AI agents)

## What this tool does

Moves or renames source files inside a project and updates every import
statement referencing them. Never breaks imports. Supported: TypeScript,
JavaScript, Java (package declaration + all importers + the file move are
kept in sync); Python/Go on the roadmap. Single binary, no LSP needed.

## When to use

- The user asks to move/rename/reorganize source files or packages.
- You refactored a directory layout and imports now point at nothing.
- You need to verify the project has no broken imports (`check`).

## Commands

### mv — move a file and rewrite its importers

```
jmove mv <source> <target> [--root DIR] [--source-root DIR] [--dry-run] [--json] [--no-git]
```

Always run `--dry-run` first and confirm the change set looks right.
Moving onto an existing path fails with `TARGET_EXISTS` — choose another
target (Phase 1 has no overwrite mode).

Git integration: inside a git repository, a tracked file is renamed with
`git mv` so the rename is staged (history-preserving `git log --follow` /
`git diff -M` work). Untracked files, non-repositories and `--no-git`
fall back to a plain filesystem rename. The import rewrites land in the
working tree unstaged either way — stage or commit them yourself.
`--json` reports the choice as `moved_via` (`"git"`/`"fs"`) and, on a
dry-run, `would_move_via`.

### check — find broken imports

```
jmove check [--root DIR] [--json]
```

Run after any move (or any edit) to validate project consistency.

### fix — auto-repair import problems

```
jmove fix [--root DIR] [--rule ID] [--dry-run] [--json]
```

Runs the deterministic rules over the whole project and applies the
repairs through the same atomic engine as `mv` (dry-run diff, rollback,
exit codes). Current Java rules: `java/unused-import` (deletes single-type
imports whose name is provably unreferenced), `java/missing-import`
(inserts the import of a project class used by simple name — unique FQN
candidate required) and `java/import-order` (Google style: statics first,
then single-type, ASCII-sorted, duplicates dropped). TS/JS:
`ts/unused-import` deletes whole statements whose every bound name is
unreferenced; mixed statements (one name live) stay untouched because ESM
imports carry module side effects, so partial specifier surgery is
deliberately off. Unknown `--rule`
fails with `INVALID_ARGUMENT` and lists the known ids. A candidate the
engine cannot prove safe is reported with `"applied": false` and a
`candidates` array of FQN options — resolve it yourself (pick one, add
the import) and re-run; `fix` never guesses. When two rules want the same
bytes (order rewrite vs an unused deletion), the urgent rule applies and
the other is deferred (`"applied": false`, "skipped") — re-run `fix`
until it reports "nothing to change" (converges in 2–3 runs).

Recommended: `fix --dry-run --json`, inspect `files[].fixes[]`
(`rule`, `line`, `message`, `applied`, optional `candidates`), then apply
without `--dry-run`. For Java projects the practical loop is: `mv` →
`fix` → `fix` again if anything was "skipped" → `check`.

## Java specifics

- Moving a `.java` file rewrites three coordinated edits: its own
  `package` declaration, every `import <fqn>` naming the class (static
  member imports keep their member suffix) and the physical move.
- The target must be a `.java` path under the same source root
  (`src/main/java`, `src`, …) and its directory maps to the new package.
  Anything else exits `1` with code `PLAN_REJECTED`.
- A class in the default package (no `package` declaration) can only be
  renamed inside its directory.
- `check` never reports unresolved Java imports (jdk, third-party,
  `pkg.*`) as broken — they are external by design, like TS bare
  specifiers.
- Monorepos with duplicate packages (`guava` vs `android/guava` under one
  `--root`): the same FQN exists in parallel trees, so resolution picks the
  sorted-first copy and a move rewrites the wrong files. Pass the global
  `--source-root DIR` to index (and fix/move) only that subtree; each tree
  is then self-contained and correct. Without the flag behavior is
  unchanged; `--source-root` on a non-directory exits 1.

## Recommended agent workflow

1. `jmove mv <src> <target> --dry-run --json` — preview.
2. Inspect `affected_files`; if unexpected, abort and ask the user.
3. `jmove mv <src> <target> --json` — apply (atomic, rolls back on error).
4. Java moves: `jmove fix --json` — heal bare same-package references
   left behind by the move (they break `javac` otherwise).
5. `jmove check --json` — verify nothing is broken.

## Output format (--json)

Every response: `{ "status": "ok" | "dry_run" | "error", "operation": "...", ... }`.
Errors carry a stable `code` (`TARGET_EXISTS`, `SOURCE_NOT_FOUND`, ...)
and a `hint` describing the next action. On success, `mv` reports
`changed_files` with line-level `old`/`new` import diffs and counters
(`moved`, `updated_imports`).

## Exit codes

- `0` — success
- `1` — operation failed (read `--json` error or stderr)
- `2` — `check` found broken imports

## Rules of use

- Never run `mv` without a prior `--dry-run` in the same session.
- After every successful `mv`, run `check`; treat exit code `2` as
  a failed refactor.
