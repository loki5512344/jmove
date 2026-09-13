# jmove — TODO

## Правила
- KISS — simplest solution that works; no speculative abstractions.
- DRY — no duplicated logic; extract into a function/module instead.
- SOLID — single responsibility per module (`cli`/`core`/`parser`/`cache`), open for extension (new languages) via the parser trait, no deps on implementation details.
- Max 250 lines per file — split when exceeded.
- Max 4 files per folder (module) — split the module when exceeded.
- Каждый коммит проходит `cargo fmt --check` и `cargo clippy -- -D warnings`.
- Запрещён mёртвый код: unused code удаляется или реализуется; `#[allow(dead_code)]` не использовать.

## MVP (Phase 1)
- [x] CLI skeleton (clap) — команды: mv, check, --dry-run, --json
- [x] Сканер файлов проекта (уважать .gitignore через `ignore` крейт)
- [x] Парсер импортов для TypeScript/JS через tree-sitter
- [x] Разрешение путей: extension guessing (.ts/.tsx/.js/...), index.* (в MVP — без этого инструмент игрушка)
- [x] Построение графа зависимостей (файл → что импортирует)
- [x] Инвертированный граф (файл → кто его импортирует)
- [x] Вычисление нового относительного пути после mv
- [x] Rewrite импортов в файлах (трогаем только specifier-строку, никогда не реформатим statement)
- [x] Атомарный apply (сначала rewrite, потом mv) + rollback при ошибке
- [x] Dry-run режим с diff выводом
- [x] Команда check (битые импорты, exit code 2) — self-test инструмента
- [x] Тесты: unit (parser, core) + e2e (CLI на фикстурах)

## AI / Agent support
- [x] --json флаг на всех командах (status ok|dry_run|error)
- [x] --dry-run + --json (preview без записи на диск)
- [x] Стабильные error codes (TARGET_EXISTS, SOURCE_NOT_FOUND, ...) + hint поле
- [x] Exit codes: 0 ok / 1 error / 2 broken imports
- [x] docs/SKILL.md — машиночитаемая документация для AI агентов
- [x] docs/EXAMPLES.md — примеры для людей и агентов

## Phase 1.5 — Java (наша ниша, аналогов в CLI нет)
- [x] tree-sitter Java грамматика: package + import extraction
- [x] Детект source root (src/main/java, src/) и соответствие package ⇄ директория
- [x] mv = три синхронных правки: package, все import в проекту, физический перенос
- [x] e2e фикстуры tests/java/

## Phase 2
- [ ] Кэш индекса на диске (bincode/rkyv) → .jmove/index
- [ ] Инкрементальная переиндексация (только изменённые файлы)
- [ ] Поддержка tsconfig paths / алиасов (@/...)
- [ ] Параллельная индексация через rayon
- [ ] --git интеграция (git mv для stage/истории)
- [ ] Перенос директорий целиком (mv папки)
- [ ] Предупреждения о не-import ссылках: package.json exports, jest mocks, tsconfig includes, markdown links
- [ ] prettier интеграция после rewrite (по желанию)

## Phase 3
- [ ] Поддержка Python (from/import, относительные точки)
- [ ] Поддержка Go (per-file, НЕ whole-package как refac)
- [ ] Команда split (авто-разбивка файла на несколько)
- [ ] Windows-пути (camino/normalize) — CI matrix

## Идеи на потом
- [ ] LSP интеграция (jmove сам как LSP server)
- [ ] Watch mode
- [ ] VS Code расширение как обёртка над CLI

## Конкуренты (см. docs/PLAN.md)
- refac / ai_refac (jav-ed): TS/Py/Rust/Go/Dart, но без dry-run, лимит 30 файлов в TS,
  Go = весь пакет, Java нет. Наш edge: dry-run+атомарность, Java, split, скорость (без LSP), UX.
