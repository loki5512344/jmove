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
- [ ] CLI skeleton (clap) — команды: mv, index, check, --dry-run
- [ ] Сканер файлов проекта (уважать .gitignore через `ignore` крейт)
- [ ] Парсер импортов для TypeScript/JS через tree-sitter
- [ ] Построение графа зависимостей (файл → что импортирует)
- [ ] Инвертированный граф (файл → кто его импортирует)
- [ ] Вычисление нового относительного пути после mv
- [ ] Rewrite импортов в файлах
- [ ] Атомарный apply (сначала rewrite, потом mv) + rollback при ошибке
- [ ] Dry-run режим с diff выводом

## Phase 2
- [ ] Кэш индекса на диске (bincode/rkyv) → .jmove/index
- [ ] Инкрементальная переиндексация (только изменённые файлы)
- [ ] Поддержка tsconfig paths / алиасов (@/...)
- [ ] Параллельная индексация через rayon

## Phase 3
- [ ] Поддержка Python
- [ ] Поддержка Java
- [ ] Поддержка Go
- [ ] Команда split (авто-разбивка файла на несколько)
- [ ] Команда check (найти все битые импорты)

## Идеи на потом
- [ ] LSP интеграция
- [ ] Watch mode
- [ ] VS Code расширение как обёртка над CLI
