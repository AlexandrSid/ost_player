# Plan: TZ fixes + NowPlaying bugfixes

**Created:** 2026-04-21  
**Orchestration:** orch-2026-04-21-15-00-tz-fixes  
**Status:** 🔄 Planning  
**Repo:** `C:/Users/Aleksandr/AI projects/ost_player`

## Goal

1) Проверить выполненность требований обновлённого ТЗ `tz/TZ_Fixes_Logging_NowPlaying_ScanDepth_TUI_Volume_Playlists_MinSizePerFolder.md`.  
2) Добавить требования багфикс-ТЗ `tz/TZ_Bugfix_NowPlaying_Navigation_And_HeaderDuplication.md`.  
3) Реализовать всё в коде и добиться соответствия acceptance criteria.

## Acceptance Criteria

### A) `TZ_Fixes_Logging_NowPlaying_ScanDepth_TUI_Volume_Playlists_MinSizePerFolder.md`

- **LOG-DEFAULT-01**: При `logging.default_level: default` в логах **нет** шумных `WARN` от зависимостей (минимум `symphonia*`, `rodio`), но есть `ERROR/FATAL`.
- **LOG-DEFAULT-02**: Доменные события изменения конфигурации логируются (создание/изменение/удаление плейлистов, изменения настроек).
- **LOG-ROTATE-01**: В `data/logs/` ротация ~10-дневными бакетами, итого **3 файла/месяц** (`YYYY-MM-01_10.log`, `YYYY-MM-11_20.log`, `YYYY-MM-21_eom.log`).
- **LOG-RETENTION-01**: Логи старше ~1 месяца удаляются при запуске (по `modified()`; retention задаётся в конфиге).
- **LOG-CONFIG-HELP-01**: В комментариях `data/config.yaml` есть справка по уровням логирования и политике приоритета (включая `RUST_LOG`).

- **NP-REENTER-01**: Если пользователь выбирает плейлист, который **уже воспроизводится**, приложение **переходит на Now Playing** без перезапуска очереди.

- **SCAN-DEPTH-01**: Есть 3 режима глубины (RootOnly / OneLevel / Recursive), корректное поведение depth=1 (корень + подпапки на 1 уровень).
- **SCAN-DEPTH-02**: Значение depth сохраняется в плейлистах и имеет обратную совместимость по YAML (legacy `root_only` читается).
- **SCAN-DEPTH-UI-01**: Иконки глубины соответствуют требованиям (`>>>`, `>|⋮`, `>⋮|`) и переключение — cycle по 3 состояниям.

- **TUI-REFRESH-01**: Обновление UI по таймеру: 1s (играет+фокус), 5s (играет xor фокус), 0s (minimized).
- **TUI-REFRESH-02**: Для корректного времени трека Now Playing периодически обновляется снапшот `PlayerSnapshot.track_position` (частота зависит от активности UI).

- **VOL-LADDER-01**: Громкость меняется по дискретному списку `audio.volume_available_percent` (тонкая шкала на малых значениях, далее шаг 5%).
- **VOL-MIGRATE-01**: Миграция: `audio.volume_step_percent` → генерация ladder при отсутствии списка; алиас `audio.default_volume_percent` → `audio.volume_default_percent`.

- **PLAYLISTS-PATH-01**: Папка `data/playlists/` больше не создаётся; используется файл `data/playlists.yaml`.

- **MIN-SIZE-FOLDER-UI-01**: В списке папок показывается `◼` (глобальный) / `🄲` (кастом) + эффективное значение (KB).
- **MIN-SIZE-FOLDER-UI-02**: Ввод кастомного `min_size` вне диапазона (10..=10000 KB) игнорируется; диапазон задан в конфиге.
- **MIN-SIZE-FOLDER-UI-03**: В status bar для выбранной папки показан эффективный `min_size` (совпадает с применяемым и с тем, что показано в списке).
- **MIN-SIZE-FOLDER-SCAN-01**: Per-folder `min_size` реально применяется при сканировании (фильтрация per-root).

- **UI-KEYHINTS-01**: Подсказки по глобальным хоткеям на экранах (Now Playing / Keys blocks) строятся из `config.yaml` → `hotkeys.bindings`.
- **UI-MENU-NUMBERS-01**: Цифровые пункты Main Menu конфигурируемые; если задан кастомный маппинг — UI выводит пункты в числовом порядке 1..9.

### B) `TZ_Bugfix_NowPlaying_Navigation_And_HeaderDuplication.md`

- **NP-HEADER-01**: На экране Now Playing отображается **один** заголовок “Now Playing” (только title блока); внутри текста первой строки “Now Playing” нет.
- **NP-NAV-PLAY-01**: Нажатие Play в главном меню **сразу** переводит на Now Playing (оптимистично), даже если скан ещё идёт.
- **NP-NAV-GUARD-01**: Если воспроизведение уже идёт и пользователь выбирает тот же плейлист — происходит Navigate to Now Playing без Stop/LoadQueue/Rescan (надёжно, без false-negative).

## Current State (high-level)

- **Already implemented / likely OK**:
  - Logging config + help preamble, suppression of noisy deps, bucket rotation (3/month), retention cleanup: `app/src/config/{mod.rs,io.rs}`, `app/src/logging.rs`.
  - Scan depth enum (RootOnly/OneLevel/Recursive) + legacy YAML compat; indexer depth limiting implemented: `app/src/config/mod.rs`, `app/src/indexer/scan.rs`, `app/src/tui/scan_indicator.rs`.
  - TUI timer refresh policy + focus/minimize -> `PlayerSetUiActivity`: `app/src/tui/terminal.rs`, `app/src/tui/action.rs`, `app/src/player/mod.rs` (command exists).
  - Volume ladder config + migration + TUI up/down via ladder: `app/src/config/mod.rs`, `app/src/tui/app.rs`.
  - `data/playlists/` dir removal: `app/src/paths.rs` (no playlists_dir), playlists path uses `data/playlists.yaml`.
  - Per-folder min_size config + normalization + per-folder scan filtering + UI markers/menu numbering in Main Menu: `app/src/config/mod.rs`, `app/src/indexer/scan.rs`, `app/src/tui/ui.rs`, `app/src/tui/app.rs`.

- **Not implemented / needs changes**:
  - Now Playing header duplicated: `app/src/tui/ui.rs` adds `"Now Playing"` as first line in content AND uses block title.
  - Bugfix-TZ navigation requirements for Play (optimistic navigate before scan completes) and reliable “already playing” detection (playback source identity) are not present (no `playback_source` in `AppState`).
  - UI key hints still hardcoded in `app/src/tui/ui.rs` (Settings/NowPlaying sections).
  - Configurable numeric menu mapping for Main Menu not found (needs design + implementation in config + UI + actions).
  - Player snapshot emission cadence based on UI activity needs verification/adjustment inside `app/src/player/mod.rs` (ensure periodic `PlayerEvent::Snapshot` at 1s/5s when playing/focused rules apply).

## Tasks (≤10)

- ⏳ **TZ-001: Gap analysis vs оба ТЗ (acceptance checklist)**  
  - Outcome: чеклист “implemented / missing / partial” по всем AC, с привязкой к файлам/модулям.
  - Touchpoints: `app/src/{logging.rs,config/*,tui/*,player/*,indexer/*,paths.rs,playlists/*}`.

- ⏳ **TZ-002: Now Playing header de-dup**  
  - Outcome: AC `NP-HEADER-01` passes; удалить первую строку “Now Playing” из контента, оставив `title("Now Playing")`.
  - Files: `app/src/tui/ui.rs`.

- ⏳ **TZ-003: Optimistic Navigate on Play (main menu)**  
  - Outcome: AC `NP-NAV-PLAY-01` passes; при `Action::PlayerLoadFromLibrary` сразу `Navigate(Screen::NowPlaying)` и статус “Scanning... (play pending)” при необходимости.
  - Files: `app/src/tui/app.rs` (+ при необходимости `app/src/tui/screens/main_menu.rs`).

- ⏳ **TZ-004: Reliable “already playing playlist” guard (playback source identity)**  
  - Outcome: AC `NP-NAV-GUARD-01` + `NP-REENTER-01` passes; добавить `playback_source` (playlist id / folders hash) и сравнение по нему вместо “строгого сравнения FolderEntry”.
  - Files: `app/src/tui/state.rs`, `app/src/tui/app.rs`.

- ⏳ **TZ-005: Player snapshot cadence tied to UI activity**  
  - Outcome: AC `TUI-REFRESH-02` passes; `playback_thread` периодически шлёт `PlayerEvent::Snapshot` с актуальным `track_position` по 1s/5s/none политике (через `SetUiActivity`).
  - Files: `app/src/player/mod.rs`, возможно `app/src/tui/terminal.rs` (если нужна тонкая настройка).

- ⏳ **TZ-006: UI key hints derived from config hotkeys.bindings**  
  - Outcome: AC `UI-KEYHINTS-01` passes; в Now Playing и других “Keys:” секциях показывать актуальные биндинги из `cfg.hotkeys.bindings` (где `None` — не отображать/помечать).
  - Files: `app/src/tui/ui.rs` (+ возможный helper в `app/src/hotkeys/mod.rs`).

- ⏳ **TZ-007: Configurable numeric main menu mapping (1..9)**  
  - Outcome: AC `UI-MENU-NUMBERS-01` passes; добавить в конфиг структуру “menu numbers mapping”, применить при рендере Main Menu и обработке цифровых действий.
  - Files: `app/src/config/mod.rs`, `app/src/config/defaults.rs`, `app/src/config/io.rs`, `app/src/tui/{ui.rs,app.rs,action.rs}`.

- ⏳ **TZ-008: Regression verification (tests + manual smoke)**  
  - Outcome: юнит/интеграционные тесты на NowPlaying навигацию, playback_source guard, key hints formatting, snapshot timing (где возможно), плюс “no-lints”.
  - Files: существующие `#[cfg(test)]` в `app/src/tui/app.rs`, `app/src/player/mod.rs` (+ новые тесты по месту).

## Risks / Default Assumptions

- **Minimize detection**: “minimized” в терминале best-effort; считаем 0x0 `Resize` достаточным и принимаем fallback “не обновлять” как соответствующий требованию.
- **Hotkeys UI**: В UI будем показывать только глобальные биндинги из `hotkeys.bindings` (как требует ТЗ); локальные TUI-клавиши (Up/Down/Enter/Esc в диалогах) остаются статичными.
- **Numeric menu mapping**: Если в YAML нет кастомного маппинга — используем текущее дефолтное распределение. При кастомном — выводим пункты строго в порядке 1..9, отсутствующие номера пропускаем.

