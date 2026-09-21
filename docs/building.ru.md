# Сборка Cronus

Инструкция по локальной сборке всех артефактов проекта: движка (CLI + TUI), общей UI-библиотеки и десктоп-приложения на Tauri.

> Проект — полиглотный монорепозиторий. В нём три независимых сборочных мира:
>
> | Мир | Инструмент | Что собирает |
> | --- | --- | --- |
> | `crates/` | Cargo (Rust workspace) | движок, `cronus` — единый бинарник (CLI; `cronus tui` открывает терминальный интерфейс как глагол того же бинарника, отдельного `cronus-tui` больше нет) |
> | `packages/` | pnpm + Vite | `@cronus/ui` — общий React-фронтенд |
> | `apps/desktop/` | pnpm + Vite + **отдельный** Cargo-проект (`apps/desktop/tauri`) | `cronus-desktop` — оболочка Tauri v2 |
>
> `apps/desktop/tauri` **намеренно отцеплён** от Rust-workspace (у него свой `Cargo.toml` с пустой секцией `[workspace]` и свой `Cargo.lock`), чтобы webview-зависимости не попадали в движок.

## 1. Требования

| Компонент | Версия | Проверка |
| --- | --- | --- |
| Rust | `1.98.1` (закреплён в `rust-toolchain.toml`) | `rustc --version` |
| компоненты Rust | `rustfmt`, `clippy` | `cargo fmt --version`, `cargo clippy --version` |
| Node.js | `>= 24` | `node --version` |
| pnpm | `12.3.4` (поле `packageManager`) | `pnpm --version` |
| C-тулчейн | MinGW-w64 GCC (сборка идёт под `x86_64-pc-windows-gnu`) | `gcc --version` **в PowerShell** |
| WebView2 Runtime | входит в Windows 10/11 | — |

`rustup` сам подхватит `1.98.1` из `rust-toolchain.toml` при первой команде
`cargo` в каталоге проекта. Если тулчейн не установлен:

```powershell
rustup toolchain install 1.98.1
rustup component add rustfmt clippy --toolchain 1.98.1
```

pnpm проще всего включить через corepack (идёт с Node):

```powershell
corepack enable
corepack prepare pnpm@12.3.4 --activate
```

### Системные зависимости Tauri

- **Windows**: WebView2 Runtime + рабочий C-компилятор (см. ниже).
- **Linux**: `webkit2gtk-4.1`, `libgtk-3-dev`, `librsvg2-dev`, `build-essential`, `libssl-dev` — по официальному списку prerequisites Tauri v2.
- **macOS**: Xcode Command Line Tools.

## 2. Первичная настройка

```powershell
git clone https://github.com/cronusio/cronus
cd cronus

# JS/TS-зависимости всего монорепо (packages/* + apps/*)
pnpm install

# Rust-тулчейн подтянется автоматически; при желании прогреть кэш:
cargo fetch
```

`pnpm install` ставит и `@tauri-apps/cli` (dev-зависимость `apps/desktop`), так что отдельная глобальная установка Tauri CLI не нужна — он вызывается как `pnpm -C apps/desktop tauri …`.

### Переносы строк

В репозитории лежит `.gitattributes` с `* text=auto eol=lf`: все текстовые файлы хранятся и выгружаются с LF независимо от `core.autocrlf`. Форматтер `biome` работает только с LF, поэтому если файлы вдруг оказались с CRLF:

```powershell
git add --renormalize .
```

## 3. Windows: собирать через PowerShell, не через Git Bash

**Все команды с нативной компиляцией C — `cargo` для `crates/` (там `rusqlite` с `bundled`), `cargo` для `apps/desktop/tauri` (шаг `windres` для `.exe`-ресурса) и любые `tauri …` — запускать в PowerShell.**

> *Причина: MSYS2-окружение Git Bash ломает загрузку `cc1.exe` mingw64 (выход 127), из-за чего `gcc` / `windres` молча падают, хотя тот же `gcc.exe` в PowerShell работает. Чистый `cargo check` из Git Bash, который внезапно падает на шаге компиляции C или ресурса, — это артефакт окружения, а не дефект кода.*

Чисто-`rustc` сборки (без свежей компиляции C) работают в любой оболочке.

Ещё нюанс PowerShell 5.1: **не** добавляйте `2>&1` при захвате вывода нативного exe — 5.1 оборачивает каждую строку stderr в `NativeCommandError`; stderr и так попадает в вывод, читайте как есть.

## 4. Сборка движка (CLI + TUI)

Из корня репозитория (PowerShell):

```powershell
# отладочная сборка всего workspace
cargo build

# релизная сборка
cargo build --release
```

Артефакты:

| Бинарь | Из крейта | Путь (release) |
| --- | --- | --- |
| `cronus.exe` | `crates/cli` (собирает и связывает `crates/tui` как библиотеку) | `target/release/cronus.exe` |

Единственный бинарник в воркспейсе — `crates/tui` больше не даёт собственный `[[bin]]`-таргет; терминальный интерфейс запускается как `cronus tui`. Собрать: `cargo build --release -p cronus-cli`.

`.cargo/config.toml` уже проставляет `CFLAGS` для обхода упаковочного бага `sqlite-vec` (отключены неиспользуемые DiskANN/rescore) — ручных действий не требуется.

## 5. Сборка десктоп-приложения

Десктоп-приложение = собранный фронтенд (`apps/desktop/dist/`) + бинарь оболочки
Tauri (`cronus-desktop.exe`). Фронтенд собирается первым; Tauri CLI делает это
сам через `beforeBuildCommand`.

### 5.1. Режим разработки

```powershell
pnpm -C apps/desktop tauri dev
```

Поднимает Vite на `http://localhost:1420` (`beforeDevCommand: pnpm dev`), затем компилирует и запускает оболочку в режиме слежения: правки фронтенда перезагружаются мгновенно, правки Rust — по пересборке.

### 5.2. Релизный бинарь — через Tauri CLI

```powershell
pnpm -C apps/desktop tauri build
```

Что происходит:

1. `beforeBuildCommand: pnpm build` → `pnpm -C apps/desktop build` (`tsc --noEmit && vite build`) → `apps/desktop/dist/`.
2. `cargo build --release` в `apps/desktop/tauri`.

Результат: `apps/desktop/tauri/target/release/cronus-desktop.exe` (~23 МБ).

В `tauri.conf.json` стоит `"bundle": { "active": false }`, поэтому инсталлятор (`.msi` / NSIS) **не** создаётся — только «сырой» exe. Чтобы получить инсталлятор, включите `bundle.active` и задайте `bundle.targets`.

### 5.3. Релизный бинарь — вручную

Полезно, когда фронтенд уже собран и нужен только Rust-шаг:

```powershell
# 1. фронтенд
pnpm -C apps/desktop build

# 2. бинарь оболочки (отдельный Cargo-проект!)
cd apps/desktop/tauri
cargo build --release
```

## 6. Гейты качества

Обязательный минимум перед тем, как считать сборку завершённой.

### Rust (по затронутому крейту или по всему workspace)

```powershell
cargo check
cargo clippy --all-targets -- -D warnings
cargo test
cargo fmt --all
# при изменениях, влияющих на производительность:
cargo bench
```

Для `apps/desktop/tauri` те же команды запускать **из каталога `apps/desktop/tauri`** (это свой workspace) и **из PowerShell**.

### Фронтенд (по затронутому пакету)

```powershell
pnpm -C packages/ui test                             # vitest
pnpm -C packages/ui exec tsc --noEmit
pnpm -C apps/desktop build                           # tsc --noEmit + vite build

pnpm exec biome check packages/ui apps/desktop/src   # линт + формат, 0 ошибок
node packages/ui/scripts/craft-lint.mjs              # «токены — единственный источник визуальной правды»

npx fallow dead-code --workspace packages/ui         # границы слоёв, мёртвый код, единственный IPC-шов
```

Из корня доступны агрегаты: `pnpm build` (= `pnpm -r build`), `pnpm test` (= `pnpm -r test`), `pnpm lint` (= `biome check --write .`), `pnpm typecheck` (= `tsc --noEmit`).

## 7. Артефакты сборки

| Что | Команда | Путь |
| --- | --- | --- |
| CLI + TUI (единый бинарник; `cronus tui` — терминальный интерфейс) | `cargo build --release -p cronus-cli` | `target/release/cronus.exe` |
| UI-библиотека (бандл) | `pnpm -C packages/ui build` | `packages/ui/dist/index.js` |
| Фронтенд десктопа | `pnpm -C apps/desktop build` | `apps/desktop/dist/` |
| Десктоп-приложение | `pnpm -C apps/desktop tauri build` | `apps/desktop/tauri/target/release/cronus-desktop.exe` |

## 8. Типовые проблемы

**`gcc` / `windres` падают с кодом 127, `gcc -E` выходит 1 без вывода.** Команда запущена в Git Bash. Перезапустите в PowerShell (см. раздел 3).

**`Cargo.lock` в `apps/desktop/tauri` постоянно пере-резолвится** (`Updating aes-gcm …`, `Adding sqlite-vec …`). Известный дрейф между `Cargo.toml` и `Cargo.lock` в этом подпроекте, к вашим изменениям отношения не имеет. После сборки:

```powershell
git checkout HEAD -- apps/desktop/tauri/Cargo.lock
```

**`cargo test` в `apps/desktop/tauri` падает с `STATUS_ENTRYPOINT_NOT_FOUND` (0xc0000139) при загрузке тест-бинаря.** Ограничение окружения на хостах с тулчейном `windows-gnu`: тест-бинарь `cronus_desktop_lib` тянет импорты `WebView2Loader` и не загружается. `cargo fmt` / `cargo clippy` при этом чистые, тесты компилируются. Запускайте юнит-тесты этого крейта в CI на не-gnu раннере (MSVC).

**`fallow audit` не завершается за разумное время на этой машине.** Локально пользуйтесь `fallow dead-code --workspace <pkg>` (границы/покрытие/вызовы, ~0.1 с); полный `fallow audit` — задача CI.

**Десктоп-приложение открывается белым экраном с текстом сверху, без стилей.** Tailwind v4 не сканирует `node_modules`, а `@cronus/ui` подключён туда симлинком рабочего пространства — утилитарные классы не генерируются. В `packages/ui/src/styles.css` после всех `@import` должна стоять строка `@source "./";` (сканировать исходники самого пакета).

**`biome check` подсвечивает все файлы как неотформатированные.** Рабочая копия выгружена с CRLF. `git add --renormalize .` (см. раздел 2).
