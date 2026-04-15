
## OST_Player — Portable TUI OST Player (Windows 11)
A lightweight, portable terminal-based music/OST player. It plays audio from your game/music folders, keeps all data next to the executable in `data/`, and supports **global hotkeys** so you can control playback even when the terminal is not focused.
- **Download (v0.1.0)**: [`ost_player.exe`](https://github.com/AlexandrSid/ost_player/releases/download/v0.1.0/ost_player.exe)
---
## Release notes (EN)
`ost_player` is a **portable TUI music/OST player** for Windows 11. It runs from a folder, stores all data next to the executable in `data/`, plays in the background, and is controlled via **global hotkeys** (works even when the terminal window is not focused).
### What it is / What it can do
- **Portable**: config/playlists/logs are stored in `./data/` next to the `.exe` (no `%APPDATA%`, no registry).
- **Library from folders**: add source folders, scan tracks, filter out small “junk” files using **`min_size` in kb**.
- **Playlists**: save/load named sets of folders, switch (swap) the active playlist.
- **OGG support**: `.ogg` playback via **`ffmpeg` fallback** (if the built-in decoder fails). If `ffmpeg` is missing, a clear error is shown and `.mp3` still works.
- **Rescan-before-play**: pressing Play triggers a rescan first, so **removed folders don’t end up in the queue**.
- **Per-folder scan mode**: toggle **root-only** (only files in the folder root) vs **recursive** (include subfolders), with a visible indicator in the folder list.
- **Volume control**: global **VolumeUp/VolumeDown** hotkeys (5% step). Current volume is shown in the status bar as `Volume=NN%`.
- **TUI UX fixes**: consistent numbered menu items, fixed path input (TextInput shows typed text and cursor).
- **Windows icon**: the `ost_player.exe` has an embedded app icon (console window icon is best-effort and depends on the terminal).
---
## Quick user guide (EN)
### Quick start
- Put `ost_player.exe` into a **writable folder** (avoid `Program Files`).
- Run it once — it will create `data/` and default config files.
- In the main menu:
  - **Add folder**: enter an absolute path to your music/OST folder.
  - **Play**: starts playback (a rescan happens before playback starts).
  - **Settings**: set `min_size` (in **kb**), `shuffle`, and `repeat`.
  - **Playlists**: create/load playlists (named folder sets).
### Playback control (default global hotkeys)
- **Play/Pause**: `Ctrl + RightShift + Up`
- **Next**: `Ctrl + RightShift + Right` (tap)
- **Previous**: `Ctrl + RightShift + Left` (tap)
- **Seek**: hold `Left/Right` (5-second step repeated)
- **Repeat toggle**: `Ctrl + RightShift + Down`
- **Shuffle toggle**: typically `Ctrl + RightShift + S` (if enabled/configured)
- **Volume**: `Ctrl + Shift + PageUp/PageDown` (5% step, clamped to `0..100%`)
Note: Windows `RegisterHotKey` does **not reliably distinguish** left vs right Ctrl/Shift. A binding like `LeftCtrl+RightShift` may effectively behave like `Ctrl+Shift`.
### Where to configure
- **`data/config.yaml`**: folders, hotkeys, `min_size_kb`, shuffle/repeat, default volume.
- **`data/playlists.yaml`**: playlists (name + folder list).
---
## Релиз-ноут (RU)
`ost_player` — **портативный TUI‑плеер OST/музыки из папок** для Windows 11. Запускается из `.exe`, хранит данные рядом в `data/`, играет в фоне и управляется **глобальными хоткеями** (работают даже без фокуса терминала).
### Что это / что умеет
- **Portable**: конфиг/плейлисты/логи в `./data/` рядом с `.exe` (не `%APPDATA%`, не реестр).
- **Библиотека из папок**: добавляете папки‑источники, треки сканируются по ним, “мусор” отсекается порогом **`min_size` в kb**.
- **Плейлисты**: сохранение/загрузка наборов папок, переключение активного плейлиста (swap).
- **OGG поддержка**: `.ogg` воспроизводятся через fallback на **`ffmpeg`** (если встроенный декодер не справился). Если `ffmpeg` отсутствует — показывается понятная ошибка, `.mp3` продолжают работать.
- **Rescan-before-play**: перед стартом воспроизведения делается перескан активных папок, чтобы **удалённые папки не попадали в очередь**.
- **Режим сканирования на папку**: для каждой папки можно переключать **root-only** (только корень) / **recursive** (с подпапками) — видно индикатором в списке.
- **Громкость**: глобальные хоткеи **VolumeUp/VolumeDown** (шаг 5%), громкость отображается в статус‑строке как `Volume=NN%`.
- **UI‑улучшения**: консистентная нумерация пунктов меню, исправлен ввод пути (TextInput — видимый текст и курсор).
- **Windows‑иконка**: у `ost_player.exe` встроена иконка приложения (а иконка окна консоли — best‑effort: зависит от терминала).
---
## Короткая инструкция (RU)
### Быстрый старт
- Положите `ost_player.exe` в **папку с правами записи** (не `Program Files`).
- Запустите — приложение создаст `data/` и конфиги по умолчанию.
- В главном меню:
  - **Add folder** → добавьте абсолютный путь к папке с музыкой.
  - **Play** → запустите воспроизведение (перед стартом будет перескан).
  - **Settings** → настройте `min_size` (в **kb**), `shuffle`, `repeat`.
  - **Playlists** → создайте/загрузите плейлист (набор папок).
### Управление во время воспроизведения (глобальные хоткеи по умолчанию)
- **Play/Pause**: `Ctrl + RightShift + Up`
- **Next**: `Ctrl + RightShift + Right` (tap)
- **Prev**: `Ctrl + RightShift + Left` (tap)
- **Перемотка**: удержание `Left/Right` (шаг 5 секунд повтором)
- **Repeat toggle**: `Ctrl + RightShift + Down`
- **Shuffle toggle**: обычно `Ctrl + RightShift + S` (если включено/настроено)
- **Громкость**: `Ctrl + Shift + PageUp/PageDown` (шаг 5%, `0..100%`)
Примечание: на Windows `RegisterHotKey` **не гарантирует** строгое различение левого/правого Ctrl/Shift — сочетание вида `LeftCtrl+RightShift` фактически может работать как `Ctrl+Shift`.
### Где это настраивается
- **`data/config.yaml`**: папки, хоткеи, `min_size_kb`, shuffle/repeat, дефолтная громкость.
- **`data/playlists.yaml`**: плейлисты (имя + список папок).
---
## Notes from the original requirements (from the TZ)
This README is based on the implemented requirements:
- portable app layout (`data/` next to the binary)
- TUI menus (folders / settings / playlists)
- global hotkeys (tap vs hold, seek step)
- size filter (`min_size`), shuffle, repeat
- per-folder scan mode (root-only vs recursive)
- rescan-before-play
- OGG playback via optional `ffmpeg` fallback
- volume hotkeys + volume in status bar
- Windows executable icon (and best-effort console icon)
