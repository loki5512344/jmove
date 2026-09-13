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
jmove mv <source> <target> [--root DIR] [--dry-run] [--json]
```

Always run `--dry-run` first and confirm the change set looks right.
Moving onto an existing path fails with `TARGET_EXISTS` — choose another
target (Phase 1 has no overwrite mode).

### check — find broken imports

```
jmove check [--root DIR] [--json]
```

Run after any move (or any edit) to validate project consistency.

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

## Recommended agent workflow

1. `jmove mv <src> <target> --dry-run --json` — preview.
2. Inspect `affected_files`; if unexpected, abort and ask the user.
3. `jmove mv <src> <target> --json` — apply (atomic, rolls back on error).
4. `jmove check --json` — verify nothing is broken.

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
