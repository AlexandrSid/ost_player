# Release Notes (EN)

**Date:** 2026-04-21  
**Scope:** TZ Backlog Milestone 1 (`TZ_Full_Backlog_EN_20260421`)

## Highlights
- **Smoother TUI resizing:** window resize triggers an immediate redraw (no waiting for tick).
- **Much quieter default logs:** DEFAULT logging is now **ERROR-only globally** with a **narrow INFO whitelist** for persistence events.
- **Playback continuity:** changing scan parameters can preserve the currently playing track when it still exists in the refreshed library.
- **Safer playlists workflow:** playlists are **explicit-save** with a dirty indicator and quit confirmation.
- **More consistent UI:** playback hotkey hints are derived from `hotkeys.bindings` on all screens.

## Changes
- **TUI**
  - Resize events always schedule redraw; 0×0 minimize remains safe.
  - Main menu UX refinements: shorter labels, dynamic column width, separate **Play / Exit** hints; numeric block no longer implies `5/0` for play/quit.
  - Playback hotkey hints unified across screens from config, with `?` placeholder for missing bindings.
- **Logging**
  - DEFAULT policy: `error` globally + `target=ost_player::persist=info` whitelist; noisy deps suppressed.
  - `RUST_LOG` remains a full override; invalid `RUST_LOG` warns **once** and falls back to config policy.
- **Playlists**
  - No autosave on mutations; changes mark playlists **dirty**.
  - Added explicit **Save** action; main menu shows “(save changes)” when dirty.
  - Quit from main menu prompts when playlists are dirty.
- **Now Playing**
  - “Play” from main menu: if already Playing/Paused and requested source matches current `PlaybackSource`, it only navigates to Now Playing (no rescan/reload).
  - After scan changes: resync queue while preserving playback when current file still exists; fallback to load when missing.
- **Indexing**
  - Added deterministic **index fingerprint** persisted in `index.yaml`.
  - Debounced background rescan infrastructure with stale-result ignore by token.
  - Cache reuse now requires matching **schema version**.
- **Config**
  - Legacy compatibility is narrowed to **`playlists.yaml` only**; `config.yaml` is current-schema-only.

## Verification
- `cargo fmt --all -- --check` ✅  
- `cargo test --all` ✅  
- `cargo clippy --all-targets --all-features -- -D warnings` ✅

## Notes / Known limitations
- Some UI alignment uses character/byte widths; exotic Unicode-width terminals may still show minor alignment drift.
- NP-002 preserves playback in the common case; sink preservation still depends on strict path equality inside the player (improved by sending the refreshed track path from the new track list).

---

# Релиз-ноты (RU)

**Дата:** 2026-04-21  
**Объём:** TZ Backlog Milestone 1 (`TZ_Full_Backlog_EN_20260421`)

## Главное
- **Ресайз TUI стал “живым”:** любое изменение размера окна вызывает немедленную перерисовку.
- **Тише логи по умолчанию:** режим DEFAULT теперь **глобально только ERROR**, а INFO — только по **узкому whitelist** для событий сохранения/персистенции.
- **Непрерывность воспроизведения:** при изменении параметров скана текущий трек сохраняется, если он всё ещё есть в обновлённой библиотеке.
- **Плейлисты безопаснее:** плейлисты стали **явно сохраняемыми** (explicit-save) с признаком “есть несохранённые изменения” и подтверждением выхода.
- **Единые подсказки хоткеев:** подсказки берутся из `hotkeys.bindings` на всех экранах.

## Изменения
- **TUI**
  - Ресайз всегда ставит флаг перерисовки; 0×0 при minimize остаётся безопасным.
  - Улучшено меню: укорочены подписи, ширина колонок подстраивается под контент, отдельный блок **Play / Exit**; цифры больше не намекают на `5/0` для play/quit.
  - Подсказки по hotkeys для playback унифицированы и берутся из конфига, отсутствующие биндинги показывают `?`.
- **Логирование**
  - DEFAULT: `error` глобально + whitelist `target=ost_player::persist=info`, шумные зависимости подавлены.
  - `RUST_LOG` по-прежнему полностью переопределяет политику; некорректный `RUST_LOG` предупреждает **один раз** и откатывается к политике из конфига.
- **Плейлисты**
  - Убраны автосейвы при изменениях; изменения помечают состояние как **dirty**.
  - Добавлено явное **Save**; в главном меню появляется “(save changes)” при dirty.
  - Выход из главного меню при dirty требует подтверждения.
- **Now Playing**
  - “Play” из главного меню: если уже Playing/Paused и источник совпадает с текущим `PlaybackSource`, происходит только переход на Now Playing (без рескана/перезагрузки очереди).
  - После изменений параметров скана: ресинк очереди с сохранением воспроизведения, если файл остался; иначе fallback на загрузку очереди.
- **Индексация**
  - Добавлен детерминированный **fingerprint** индекса, сохраняется в `index.yaml`.
  - Дебаунс фоновых пересканов + игнор “устаревших” результатов по токену.
  - Повторное использование кэша теперь требует совпадения **версии схемы**.
- **Конфиг**
  - Legacy-совместимость оставлена **только для `playlists.yaml`**; `config.yaml` — только актуальная схема.

## Проверка качества
- `cargo fmt --all -- --check` ✅  
- `cargo test --all` ✅  
- `cargo clippy --all-targets --all-features -- -D warnings` ✅

## Примечания
- В некоторых местах выравнивание зависит от ширины символов; в терминалах с нестандартной Unicode-шириной возможны небольшие смещения.
- В NP-002 сохранение sink всё ещё опирается на строгое сравнение путей внутри плеера (улучшено тем, что в плеер отправляется “обновлённый” путь из нового списка треков).

