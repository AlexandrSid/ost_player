use crate::command_bus::{BusMessage, CommandEnvelope, CommandSource};
use crate::config::HotkeysConfig;
use crate::hotkeys::logic::{HotkeysEngine, KeyDirection, KeyEvent};
use crate::tui::action::Action;
use std::sync::mpsc;

pub mod hints;
pub mod logic;

pub struct HotkeysService {
    #[cfg(windows)]
    #[expect(dead_code)]
    inner: windows::WindowsHotkeysService,
}

impl HotkeysService {
    /// Start global hotkeys. On non-Windows platforms this returns `Ok(None)`.
    ///
    /// Errors are returned as user-friendly strings (caller can surface them in the UI).
    pub fn start(
        cfg: &HotkeysConfig,
        tx: mpsc::Sender<BusMessage>,
    ) -> Result<Option<Self>, String> {
        #[cfg(windows)]
        {
            let inner = windows::WindowsHotkeysService::start(cfg, tx)?;
            Ok(Some(Self { inner }))
        }
        #[cfg(not(windows))]
        {
            let _ = cfg;
            let _ = tx;
            Ok(None)
        }
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use crate::config::{HotkeyChord, HotkeyKey, HotkeyModifier, TapHoldBinding};
    use ::windows::Win32::Foundation::{GetLastError, LPARAM, WPARAM};
    use ::windows::Win32::System::Threading::GetCurrentThreadId;
    use ::windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT,
        MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, VK_DOWN, VK_LEFT, VK_NEXT, VK_PRIOR,
        VK_RIGHT, VK_S, VK_SPACE, VK_UP,
    };
    use ::windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, PostThreadMessageW, TranslateMessage, MSG, WM_HOTKEY,
        WM_QUIT,
    };
    use std::collections::{HashMap, HashSet};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    pub struct WindowsHotkeysService {
        thread_id: u32,
        stop: Arc<AtomicBool>,
        join: Option<thread::JoinHandle<()>>,
    }

    impl WindowsHotkeysService {
        pub fn start(cfg: &HotkeysConfig, tx: mpsc::Sender<BusMessage>) -> Result<Self, String> {
            // RegisterHotKey-based backend.
            //
            // - Tap-only actions are emitted immediately on WM_HOTKEY.
            // - Next/Prev use tap-vs-hold logic via `HotkeysEngine`, with a poll worker that
            //   detects release using GetAsyncKeyState and calls engine.tick for hold repeats.
            let registration_plan = build_registration_plan(cfg)?;
            if registration_plan.is_empty() {
                return Err("no hotkey bindings configured".to_string());
            }

            let engine = Arc::new(Mutex::new(HotkeysEngine::from_config(cfg)));

            let stop = Arc::new(AtomicBool::new(false));
            let stop_thread = stop.clone();

            let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, String>>();
            let join = thread::spawn(move || {
                let thread_id = unsafe { GetCurrentThreadId() };
                let res = run_hotkeys_thread(
                    thread_id,
                    stop_thread,
                    tx,
                    engine,
                    registration_plan,
                    ready_tx.clone(),
                );

                // If hook install failed before readiness, propagate error.
                if res.is_err() {
                    let _ = ready_tx.send(Err(res.clone().unwrap_err()));
                }
            });

            let thread_id = match ready_rx.recv_timeout(Duration::from_secs(2)) {
                Ok(Ok(tid)) => tid,
                Ok(Err(e)) => return Err(e),
                Err(_) => return Err("timed out starting hotkeys thread".to_string()),
            };

            Ok(Self {
                thread_id,
                stop,
                join: Some(join),
            })
        }
    }

    impl Drop for WindowsHotkeysService {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            unsafe {
                let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
            if let Some(j) = self.join.take() {
                let _ = j.join();
            }
        }
    }

    #[derive(Debug, Clone)]
    enum BindingKind {
        TapOnly(Action),
        TapHold { key: HotkeyKey },
    }

    #[derive(Debug, Clone)]
    struct Registration {
        id: i32,
        chord: HotkeyChord,
        kind: BindingKind,
        label: &'static str,
    }

    fn build_registration_plan(cfg: &HotkeysConfig) -> Result<Vec<Registration>, String> {
        let mut out = Vec::new();
        let mut next_id: i32 = 1;
        let b = &cfg.bindings;

        if let Some(chord) = &b.play_pause {
            out.push(Registration {
                id: next_id,
                chord: chord.clone(),
                kind: BindingKind::TapOnly(Action::PlayerTogglePlayPause),
                label: "play_pause",
            });
            next_id += 1;
        }
        if let Some(chord) = &b.repeat_toggle {
            out.push(Registration {
                id: next_id,
                chord: chord.clone(),
                kind: BindingKind::TapOnly(Action::CycleRepeat),
                label: "repeat_toggle",
            });
            next_id += 1;
        }
        if let Some(chord) = &b.shuffle_toggle {
            out.push(Registration {
                id: next_id,
                chord: chord.clone(),
                kind: BindingKind::TapOnly(Action::ToggleShuffle),
                label: "shuffle_toggle",
            });
            next_id += 1;
        }
        if let Some(bind) = &b.next {
            out.push(reg_next_prev(next_id, true, bind));
            next_id += 1;
        }
        if let Some(bind) = &b.prev {
            out.push(reg_next_prev(next_id, false, bind));
            next_id += 1;
        }
        if let Some(chord) = &b.volume_up {
            out.push(Registration {
                id: next_id,
                chord: chord.clone(),
                kind: BindingKind::TapOnly(Action::VolumeUp),
                label: "volume_up",
            });
            next_id += 1;
        }
        if let Some(chord) = &b.volume_down {
            out.push(Registration {
                id: next_id,
                chord: chord.clone(),
                kind: BindingKind::TapOnly(Action::VolumeDown),
                label: "volume_down",
            });
        }
        Ok(out)
    }

    fn reg_next_prev(id: i32, is_next: bool, bind: &TapHoldBinding) -> Registration {
        Registration {
            id,
            chord: bind.chord.clone(),
            kind: BindingKind::TapHold {
                key: bind.chord.key,
            },
            label: if is_next { "next" } else { "prev" },
        }
    }

    fn key_to_vk(k: HotkeyKey) -> u32 {
        match k {
            HotkeyKey::Up => VK_UP.0 as u32,
            HotkeyKey::Down => VK_DOWN.0 as u32,
            HotkeyKey::Left => VK_LEFT.0 as u32,
            HotkeyKey::Right => VK_RIGHT.0 as u32,
            HotkeyKey::Space => VK_SPACE.0 as u32,
            HotkeyKey::PageUp => VK_PRIOR.0 as u32,
            HotkeyKey::PageDown => VK_NEXT.0 as u32,
            HotkeyKey::S => VK_S.0 as u32,
        }
    }

    fn chord_mod_flags(chord: &HotkeyChord) -> u32 {
        let mut flags = MOD_NOREPEAT.0;
        for m in &chord.modifiers {
            flags |= match m {
                HotkeyModifier::Ctrl | HotkeyModifier::LeftCtrl => MOD_CONTROL.0,
                HotkeyModifier::Alt => MOD_ALT.0,
                HotkeyModifier::Shift | HotkeyModifier::LeftShift | HotkeyModifier::RightShift => {
                    MOD_SHIFT.0
                }
                HotkeyModifier::Win => MOD_WIN.0,
            };
        }
        flags
    }

    fn vk_is_down(vk: i32) -> bool {
        // High-order bit set means key currently down.
        ((unsafe { GetAsyncKeyState(vk) } as u16) & 0x8000) != 0
    }

    fn snapshot_modifiers() -> HashSet<HotkeyModifier> {
        // Best-effort; for engine chord matching "extra modifiers allowed", this is sufficient.
        let mut out = HashSet::new();
        if vk_is_down(0x11) {
            out.insert(HotkeyModifier::Ctrl);
        }
        // VK_LCONTROL
        if vk_is_down(0xA2) {
            out.insert(HotkeyModifier::LeftCtrl);
        }
        if vk_is_down(0x12) {
            out.insert(HotkeyModifier::Alt);
        }
        if vk_is_down(0x10) {
            out.insert(HotkeyModifier::Shift);
        }
        // VK_LSHIFT/VK_RSHIFT
        if vk_is_down(0xA0) {
            out.insert(HotkeyModifier::LeftShift);
        }
        if vk_is_down(0xA1) {
            out.insert(HotkeyModifier::RightShift);
        }
        // VK_LWIN/VK_RWIN
        if vk_is_down(0x5B) || vk_is_down(0x5C) {
            out.insert(HotkeyModifier::Win);
        }
        out
    }

    fn is_chord_still_down(chord: &HotkeyChord) -> bool {
        if !vk_is_down(key_to_vk(chord.key) as i32) {
            return false;
        }
        let mods = snapshot_modifiers();
        chord.modifiers.iter().all(|m| mods.contains(m))
    }

    fn runtime_chord_matches_snapshot(chord: &HotkeyChord) -> bool {
        // Best-effort runtime validation for RegisterHotKey backend:
        // WM_HOTKEY does not distinguish left/right at registration time,
        // so we re-check the actual key state here using GetAsyncKeyState-derived snapshot.
        let mods = snapshot_modifiers();
        HotkeysEngine::chord_matches(chord, chord.key, &mods)
    }

    fn run_hotkeys_thread(
        thread_id: u32,
        stop: Arc<AtomicBool>,
        tx: mpsc::Sender<BusMessage>,
        engine: Arc<Mutex<HotkeysEngine>>,
        registration_plan: Vec<Registration>,
        ready_tx: mpsc::Sender<Result<u32, String>>,
    ) -> Result<u32, String> {
        // Conflicts are reported but do not prevent service from starting if at least one binding registers.
        let mut registrations: HashMap<i32, Registration> = HashMap::new();
        let mut registered_ids: Vec<i32> = Vec::new();
        let mut errors: Vec<String> = Vec::new();

        for reg in registration_plan.into_iter() {
            let vk = key_to_vk(reg.chord.key);
            let mods = chord_mod_flags(&reg.chord);
            let ok = unsafe { RegisterHotKey(None, reg.id, HOT_KEY_MODIFIERS(mods), vk) }.is_ok();
            if ok {
                registered_ids.push(reg.id);
                registrations.insert(reg.id, reg);
            } else {
                let e = unsafe { GetLastError() };
                errors.push(format!(
                    "hotkey '{}' could not be registered (vk={vk}, mods={mods:#x}): {e:?}",
                    reg.label
                ));
            }
        }

        if registered_ids.is_empty() {
            return Err(errors
                .first()
                .cloned()
                .unwrap_or_else(|| "no hotkeys could be registered".to_string()));
        }

        if !errors.is_empty() {
            let summary = format!(
                "some hotkeys could not be registered:\n- {}",
                errors.join("\n- ")
            );
            let _ = tx.send(BusMessage::Command(CommandEnvelope {
                action: Action::SetStatus(summary),
                source: CommandSource::Hotkey,
            }));
        }

        // Signal readiness (thread is alive; message loop will run).
        let _ = ready_tx.send(Ok(thread_id));

        let mut msg = MSG::default();
        loop {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            let r = unsafe { GetMessageW(&mut msg, None, 0, 0) };
            if r.0 == -1 || msg.message == WM_QUIT {
                break;
            }

            if msg.message == WM_HOTKEY {
                let id = msg.wParam.0 as i32;
                if let Some(reg) = registrations.get(&id).cloned() {
                    match reg.kind {
                        BindingKind::TapOnly(action) => {
                            if !runtime_chord_matches_snapshot(&reg.chord) {
                                // Ignore spurious WM_HOTKEY where the registered generic modifiers
                                // (e.g. MOD_CONTROL|MOD_SHIFT) match, but the configured chord
                                // requires a specific left/right modifier combination.
                                continue;
                            }
                            let _ = tx.send(BusMessage::Command(CommandEnvelope {
                                action,
                                source: CommandSource::Hotkey,
                            }));
                        }
                        BindingKind::TapHold { key } => {
                            // Spawn a worker that polls key/modifiers until release, feeding the engine.
                            let tx2 = tx.clone();
                            let chord = reg.chord.clone();
                            let stop2 = stop.clone();
                            let engine2 = engine.clone();
                            thread::spawn(move || {
                                run_tap_hold_worker(stop2, tx2, engine2, key, chord);
                            });
                        }
                    }
                }
            }

            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        // Clean shutdown: unregister all hotkeys.
        for id in registered_ids {
            unsafe {
                let _ = UnregisterHotKey(None, id);
            }
        }
        Ok(thread_id)
    }

    fn run_tap_hold_worker(
        stop: Arc<AtomicBool>,
        tx: mpsc::Sender<BusMessage>,
        engine: Arc<Mutex<HotkeysEngine>>,
        key: HotkeyKey,
        chord: HotkeyChord,
    ) {
        let started = Instant::now();
        let to_ms = |t: Instant| -> u64 { t.duration_since(started).as_millis() as u64 };

        // On "press": feed engine a Down event.
        let mods0 = snapshot_modifiers();
        let _ = engine.lock().ok().map(|mut e| {
            let _ = e.handle_event(KeyEvent {
                now_ms: 0,
                key,
                direction: KeyDirection::Down,
                modifiers_down: mods0.clone(),
            });
        });

        // While key is still down, tick the engine for hold/repeat emissions.
        loop {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            if !is_chord_still_down(&chord) {
                break;
            }

            let now_ms = to_ms(Instant::now());
            let mut mods_map: HashMap<HotkeyKey, HashSet<HotkeyModifier>> = HashMap::new();
            mods_map.insert(key, snapshot_modifiers());

            if let Ok(mut e) = engine.lock() {
                for a in e.tick(now_ms, &mods_map) {
                    let _ = tx.send(BusMessage::Command(CommandEnvelope {
                        action: a,
                        source: CommandSource::Hotkey,
                    }));
                }
            }
            thread::sleep(Duration::from_millis(10));
        }

        // On release: feed engine an Up event (may emit tap).
        let now_ms = to_ms(Instant::now());
        let mods_up = snapshot_modifiers();
        if let Ok(mut e) = engine.lock() {
            for a in e.handle_event(KeyEvent {
                now_ms,
                key,
                direction: KeyDirection::Up,
                modifiers_down: mods_up,
            }) {
                let _ = tx.send(BusMessage::Command(CommandEnvelope {
                    action: a,
                    source: CommandSource::Hotkey,
                }));
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::config::{
            HotkeyChord, HotkeyKey, HotkeyModifier, HotkeysBindings, HotkeysConfig,
        };

        fn cfg_with_bindings(b: HotkeysBindings) -> HotkeysConfig {
            HotkeysConfig {
                timings: Default::default(),
                bindings: b,
            }
        }

        fn chord(mods: &[HotkeyModifier], key: HotkeyKey) -> HotkeyChord {
            HotkeyChord {
                modifiers: mods.to_vec(),
                key,
            }
        }

        #[test]
        fn registration_plan_includes_volume_bindings_when_some() {
            let cfg = cfg_with_bindings(HotkeysBindings {
                volume_up: Some(chord(
                    &[HotkeyModifier::LeftCtrl, HotkeyModifier::RightShift],
                    HotkeyKey::PageUp,
                )),
                volume_down: Some(chord(
                    &[HotkeyModifier::LeftCtrl, HotkeyModifier::RightShift],
                    HotkeyKey::PageDown,
                )),
                ..Default::default()
            });

            let plan = build_registration_plan(&cfg).expect("plan builds");
            let labels: std::collections::HashSet<&'static str> =
                plan.iter().map(|r| r.label).collect();

            assert!(labels.contains("volume_up"));
            assert!(labels.contains("volume_down"));
        }

        #[test]
        fn registration_plan_excludes_volume_bindings_when_none() {
            let cfg = cfg_with_bindings(HotkeysBindings {
                volume_up: None,
                volume_down: None,
                ..Default::default()
            });

            let plan = build_registration_plan(&cfg).expect("plan builds");
            let labels: std::collections::HashSet<&'static str> =
                plan.iter().map(|r| r.label).collect();

            assert!(!labels.contains("volume_up"));
            assert!(!labels.contains("volume_down"));
        }

        #[test]
        fn registration_plan_can_include_only_one_volume_binding() {
            let cfg = cfg_with_bindings(HotkeysBindings {
                volume_up: Some(chord(
                    &[HotkeyModifier::LeftCtrl, HotkeyModifier::RightShift],
                    HotkeyKey::PageUp,
                )),
                volume_down: None,
                ..Default::default()
            });

            let plan = build_registration_plan(&cfg).expect("plan builds");
            let labels: std::collections::HashSet<&'static str> =
                plan.iter().map(|r| r.label).collect();

            assert!(labels.contains("volume_up"));
            assert!(!labels.contains("volume_down"));
        }
    }
}
