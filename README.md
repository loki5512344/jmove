# jmove

**Move source files. Keep every import intact.**

`jmove` is a fast, single-binary CLI that moves/renames source files in a
project and rewrites all import statements that reference them — no IDE,
no language server, no runtime dependencies. Built for humans *and* AI
agents.

## Why

`git mv` moves the file but breaks every relative import pointing at it.
IDE renames fix imports but need an IDE. Existing CLI alternatives either
shell out to heavy language servers (bun/ts-morph, Rope, gopls,
rust-analyzer) or refuse to preview what they will change.

jmove's answer:

- **Dry-run + unified diff** — see every change before it hits disk.
- **Atomic apply with rollback** — imports are rewritten first, the file
  moves last; any failure restores everything.
- **AI-agent mode** — `--json` everywhere, stable error codes, exit codes,
  and a machine-readable skill doc (`docs/SKILL.md`).
- **Pure Rust, one binary** — no external toolchains to install.
- **Java support** — the package/directory coupling nobody else automates
  outside IntelliJ.

## Install

```sh
# From a clone of this repo (installs `jmove` into ~/.cargo/bin, on PATH):
cargo install --path . --locked

# Latest published version (once jmove is on crates.io — not yet published):
cargo install jmove
```

Use as a library (the CLI and the engine are separate targets; `jmove::core`
holds indexing/planning/apply, `jmove::parser` the language frontends):

```toml
[dependencies]
jmove = { git = "https://github.com/loki5512344/jmove" }
# or, for a local checkout:  jmove = { path = "../jmove" }
```

## Usage

```sh
# Preview what a move would change
jmove mv src/utils/parser.ts src/core/parser.ts --dry-run

# Apply it
jmove mv src/utils/parser.ts src/core/parser.ts

# Inside a git repo a tracked file moves via `git mv` (staged, history kept);
# --no-git forces a plain rename
jmove mv src/foo.ts src/bar/foo.ts --no-git

# Java: jmove updates `package`, all `import`s and moves the file
jmove mv src/com/example/utils/Parser.java src/com/example/core/Parser.java

# Find broken imports (exit code 2 if any)
jmove check

# Auto-repair import problems (unused + missing imports today) — same dry-run/atomic engine
jmove fix --dry-run
jmove fix

# AI-agent workflow
jmove mv src/foo.ts src/bar/foo.ts --dry-run --json
jmove mv src/foo.ts src/bar/foo.ts --json
jmove check --json
```

See [docs/EXAMPLES.md](docs/EXAMPLES.md) for more, and
[docs/SKILL.md](docs/SKILL.md) if you are an AI agent.

## Roadmap

TypeScript/JavaScript and Java (the open niche) are in — real-world tested
on `google/guava`. `fix` auto-repairs small breakages on the same
dry-run/atomic engine (Java: unused, missing and
misordered imports; TS: unused imports), then Python, Go. `split` (automatic file decomposition) is
planned — no tool does it. Full plan: [docs/PLAN.md](docs/PLAN.md).

## License

GPL-3.0-only. See [LICENSE](LICENSE).
