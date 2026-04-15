use crate::config::{HotkeyChord, HotkeyHoldAction, HotkeyKey, HotkeyModifier, HotkeysConfig};
use crate::tui::action::Action;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
struct RuntimeBinding {
    chord: HotkeyChord,
    tap: Action,
    hold: Option<HotkeyHoldAction>,
}

#[derive(Debug, Clone)]
struct PressState {
    started_ms: u64,
    hold_fired: bool,
    next_hold_or_repeat_ms: Option<u64>,
}

/// Pure hotkey logic engine.
///
/// - No OS hooks, no threads, no sleeping.
/// - Feed key down/up events and advance time via `tick`.
/// - Emits actions for taps and holds (with repeat).
#[derive(Debug, Clone)]
pub struct HotkeysEngine {
    timings: crate::config::HotkeysTimings,
    bindings: Vec<RuntimeBinding>,
    presses: HashMap<HotkeyKey, PressState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyDirection {
    Down,
    Up,
}

#[derive(Debug, Clone)]
pub struct KeyEvent {
    pub now_ms: u64,
    pub key: HotkeyKey,
    pub direction: KeyDirection,
    /// Snapshot of currently pressed modifiers at the time of the event.
    pub modifiers_down: HashSet<HotkeyModifier>,
}

impl HotkeysEngine {
    pub fn from_config(cfg: &HotkeysConfig) -> Self {
        let mut bindings = Vec::new();
        let b = &cfg.bindings;
        if let Some(chord) = &b.play_pause {
            bindings.push(RuntimeBinding {
                chord: chord.clone(),
                tap: Action::PlayerTogglePlayPause,
                hold: None,
            });
        }
        if let Some(chord) = &b.repeat_toggle {
            bindings.push(RuntimeBinding {
                chord: chord.clone(),
                tap: Action::CycleRepeat,
                hold: None,
            });
        }
        if let Some(chord) = &b.shuffle_toggle {
            bindings.push(RuntimeBinding {
                chord: chord.clone(),
                tap: Action::ToggleShuffle,
                hold: None,
            });
        }
        if let Some(bind) = &b.next {
            bindings.push(RuntimeBinding {
                chord: bind.chord.clone(),
                tap: Action::PlayerNext,
                hold: bind.hold.clone(),
            });
        }
        if let Some(bind) = &b.prev {
            bindings.push(RuntimeBinding {
                chord: bind.chord.clone(),
                tap: Action::PlayerPrev,
                hold: bind.hold.clone(),
            });
        }

        Self {
            timings: cfg.timings.clone(),
            bindings,
            presses: HashMap::new(),
        }
    }

    pub fn bindings_len(&self) -> usize {
        self.bindings.len()
    }

    pub fn chord_matches(
        chord: &HotkeyChord,
        key: HotkeyKey,
        down_mods: &HashSet<HotkeyModifier>,
    ) -> bool {
        chord.key == key && chord.modifiers.iter().all(|m| down_mods.contains(m))
    }

    /// Feed a key down/up event. Actions may be emitted on key up (tap),
    /// while hold actions are emitted only via `tick`.
    pub fn handle_event(&mut self, ev: KeyEvent) -> Vec<Action> {
        match ev.direction {
            KeyDirection::Down => self.handle_down(ev.now_ms, ev.key, &ev.modifiers_down),
            KeyDirection::Up => self.handle_up(ev.now_ms, ev.key, &ev.modifiers_down),
        }
    }

    fn handle_down(
        &mut self,
        now_ms: u64,
        key: HotkeyKey,
        down_mods: &HashSet<HotkeyModifier>,
    ) -> Vec<Action> {
        // Start tracking only if there is a matching binding.
        let Some(binding) = self
            .bindings
            .iter()
            .find(|b| Self::chord_matches(&b.chord, key, down_mods))
            .cloned()
        else {
            return Vec::new();
        };

        if self.presses.contains_key(&key) {
            return Vec::new();
        }

        let next = binding
            .hold
            .as_ref()
            .map(|_| now_ms.saturating_add(self.timings.hold_threshold_ms));
        self.presses.insert(
            key,
            PressState {
                started_ms: now_ms,
                hold_fired: false,
                next_hold_or_repeat_ms: next,
            },
        );
        Vec::new()
    }

    fn handle_up(
        &mut self,
        now_ms: u64,
        key: HotkeyKey,
        down_mods: &HashSet<HotkeyModifier>,
    ) -> Vec<Action> {
        let Some(press) = self.presses.remove(&key) else {
            return Vec::new();
        };

        // If hold has fired, never tap.
        if press.hold_fired {
            return Vec::new();
        }

        // Defensive: if released after threshold but tick never ran, treat as hold (no tap),
        // matching the intended UX (avoid accidental "next" when user held long).
        let held_ms = now_ms.saturating_sub(press.started_ms);
        if held_ms >= self.timings.hold_threshold_ms {
            return Vec::new();
        }

        for b in &self.bindings {
            if Self::chord_matches(&b.chord, key, down_mods) {
                return vec![b.tap.clone()];
            }
        }
        Vec::new()
    }

    /// Advance internal timers. Returns hold-repeat actions that should be emitted at `now_ms`.
    pub fn tick(
        &mut self,
        now_ms: u64,
        modifiers_down: &HashMap<HotkeyKey, HashSet<HotkeyModifier>>,
    ) -> Vec<Action> {
        let mut out = Vec::new();

        // Iterate over a stable key list so we can mutate `presses`.
        let keys: Vec<HotkeyKey> = self.presses.keys().copied().collect();
        for key in keys {
            let Some(st) = self.presses.get_mut(&key) else {
                continue;
            };
            let Some(next_ms) = st.next_hold_or_repeat_ms else {
                continue;
            };
            if now_ms < next_ms {
                continue;
            }

            // Find binding + modifiers snapshot to validate chord still matches.
            let Some(down_mods) = modifiers_down.get(&key) else {
                continue;
            };
            let Some(binding) = self
                .bindings
                .iter()
                .find(|b| Self::chord_matches(&b.chord, key, down_mods))
                .cloned()
            else {
                continue;
            };
            let Some(hold_action) = binding.hold else {
                continue;
            };

            // Fire hold (first time) or repeats (subsequent).
            st.hold_fired = true;

            // Catch up in case ticks are sparse.
            let mut t = next_ms;
            while t <= now_ms {
                out.push(hold_action_to_action(&hold_action, &self.timings));
                t = t.saturating_add(self.timings.repeat_interval_ms);
            }
            st.next_hold_or_repeat_ms = Some(t);
        }

        out
    }
}

fn hold_action_to_action(
    hold: &HotkeyHoldAction,
    timings: &crate::config::HotkeysTimings,
) -> Action {
    match hold {
        HotkeyHoldAction::SeekStep { direction } => {
            let seconds = (*direction) * timings.seek_step_seconds as i64;
            Action::PlayerSeekRelativeSeconds(seconds)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        HotkeyChord, HotkeyHoldAction, HotkeyKey, HotkeyModifier, HotkeysBindings, HotkeysConfig,
        HotkeysTimings, TapHoldBinding,
    };

    fn mods(ms: &[HotkeyModifier]) -> HashSet<HotkeyModifier> {
        ms.iter().copied().collect()
    }

    fn key_mods_map(
        key: HotkeyKey,
        ms: &[HotkeyModifier],
    ) -> HashMap<HotkeyKey, HashSet<HotkeyModifier>> {
        let mut m = HashMap::new();
        m.insert(key, mods(ms));
        m
    }

    #[test]
    fn tap_emits_action_on_key_up_before_hold_threshold() {
        let cfg = HotkeysConfig {
            timings: HotkeysTimings {
                hold_threshold_ms: 300,
                repeat_interval_ms: 250,
                seek_step_seconds: 5,
            },
            bindings: HotkeysBindings {
                play_pause: Some(HotkeyChord {
                    modifiers: vec![HotkeyModifier::Ctrl],
                    key: HotkeyKey::Space,
                }),
                ..Default::default()
            },
        };

        let mut e = HotkeysEngine::from_config(&cfg);
        assert_eq!(e.bindings_len(), 1);

        let down = e.handle_event(KeyEvent {
            now_ms: 0,
            key: HotkeyKey::Space,
            direction: KeyDirection::Down,
            modifiers_down: mods(&[HotkeyModifier::Ctrl]),
        });
        assert!(down.is_empty());

        let up = e.handle_event(KeyEvent {
            now_ms: 100,
            key: HotkeyKey::Space,
            direction: KeyDirection::Up,
            modifiers_down: mods(&[HotkeyModifier::Ctrl]),
        });
        assert_eq!(up, vec![Action::PlayerTogglePlayPause]);
    }

    #[test]
    fn hold_emits_seek_actions_on_tick_and_never_taps() {
        let cfg = HotkeysConfig {
            timings: HotkeysTimings {
                hold_threshold_ms: 300,
                repeat_interval_ms: 250,
                seek_step_seconds: 5,
            },
            bindings: HotkeysBindings {
                next: Some(TapHoldBinding {
                    chord: HotkeyChord {
                        modifiers: vec![HotkeyModifier::Ctrl],
                        key: HotkeyKey::Right,
                    },
                    hold: Some(HotkeyHoldAction::SeekStep { direction: 1 }),
                }),
                ..Default::default()
            },
        };

        let mut e = HotkeysEngine::from_config(&cfg);
        let key = HotkeyKey::Right;

        e.handle_event(KeyEvent {
            now_ms: 0,
            key,
            direction: KeyDirection::Down,
            modifiers_down: mods(&[HotkeyModifier::Ctrl]),
        });

        // Before threshold: nothing.
        let out = e.tick(299, &key_mods_map(key, &[HotkeyModifier::Ctrl]));
        assert!(out.is_empty());

        // At threshold: first hold emission.
        let out = e.tick(300, &key_mods_map(key, &[HotkeyModifier::Ctrl]));
        assert_eq!(out, vec![Action::PlayerSeekRelativeSeconds(5)]);

        // Sparse ticks should "catch up" repeats.
        // hold_threshold=300, repeat_interval=250 => nexts at 550, 800, ...
        let out = e.tick(800, &key_mods_map(key, &[HotkeyModifier::Ctrl]));
        assert_eq!(
            out,
            vec![
                Action::PlayerSeekRelativeSeconds(5),
                Action::PlayerSeekRelativeSeconds(5),
            ]
        );

        // On release: no tap.
        let up = e.handle_event(KeyEvent {
            now_ms: 900,
            key,
            direction: KeyDirection::Up,
            modifiers_down: mods(&[HotkeyModifier::Ctrl]),
        });
        assert!(up.is_empty());
    }

    #[test]
    fn release_after_threshold_without_tick_does_not_tap() {
        let cfg = HotkeysConfig {
            timings: HotkeysTimings {
                hold_threshold_ms: 300,
                repeat_interval_ms: 250,
                seek_step_seconds: 5,
            },
            bindings: HotkeysBindings {
                next: Some(TapHoldBinding {
                    chord: HotkeyChord {
                        modifiers: vec![HotkeyModifier::Ctrl],
                        key: HotkeyKey::Right,
                    },
                    hold: Some(HotkeyHoldAction::SeekStep { direction: 1 }),
                }),
                ..Default::default()
            },
        };

        let mut e = HotkeysEngine::from_config(&cfg);
        let key = HotkeyKey::Right;

        e.handle_event(KeyEvent {
            now_ms: 0,
            key,
            direction: KeyDirection::Down,
            modifiers_down: mods(&[HotkeyModifier::Ctrl]),
        });

        // Never call tick; release after threshold.
        let up = e.handle_event(KeyEvent {
            now_ms: 300,
            key,
            direction: KeyDirection::Up,
            modifiers_down: mods(&[HotkeyModifier::Ctrl]),
        });
        assert!(up.is_empty());
    }
}
