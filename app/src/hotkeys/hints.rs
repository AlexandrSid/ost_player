use crate::config::{HotkeyChord, HotkeyKey, HotkeyModifier, TapHoldBinding};

fn modifier_rank(m: HotkeyModifier) -> u8 {
    match m {
        HotkeyModifier::Ctrl => 10,
        HotkeyModifier::LeftCtrl => 11,
        HotkeyModifier::Alt => 20,
        HotkeyModifier::Shift => 30,
        HotkeyModifier::LeftShift => 31,
        HotkeyModifier::RightShift => 32,
        HotkeyModifier::Win => 40,
    }
}

fn modifier_label(m: HotkeyModifier) -> &'static str {
    match m {
        HotkeyModifier::Ctrl => "Ctrl",
        HotkeyModifier::Alt => "Alt",
        HotkeyModifier::Shift => "Shift",
        HotkeyModifier::Win => "Win",
        HotkeyModifier::LeftCtrl => "LCtrl",
        HotkeyModifier::LeftShift => "LShift",
        HotkeyModifier::RightShift => "RShift",
    }
}

fn key_label(k: HotkeyKey) -> &'static str {
    match k {
        HotkeyKey::Up => "Up",
        HotkeyKey::Down => "Down",
        HotkeyKey::Left => "Left",
        HotkeyKey::Right => "Right",
        HotkeyKey::Space => "Space",
        HotkeyKey::PageUp => "PageUp",
        HotkeyKey::PageDown => "PageDown",
        HotkeyKey::S => "S",
    }
}

/// Format a configured hotkey chord into a compact, stable UI hint (e.g. `Ctrl+Alt+S`).
pub fn format_chord(chord: &HotkeyChord) -> String {
    let mut mods = chord.modifiers.clone();
    mods.sort_by_key(|m| modifier_rank(*m));
    mods.dedup();

    let mut out = String::new();
    for (idx, m) in mods.iter().enumerate() {
        if idx > 0 {
            out.push('+');
        }
        out.push_str(modifier_label(*m));
    }
    if !out.is_empty() {
        out.push('+');
    }
    out.push_str(key_label(chord.key));
    out
}

/// Format a tap/hold binding. Currently the UI hints show the chord only.
pub fn format_tap_hold_binding(b: &TapHoldBinding) -> String {
    format_chord(&b.chord)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_chord_sorts_and_dedups_modifiers() {
        let chord = HotkeyChord {
            modifiers: vec![
                HotkeyModifier::Shift,
                HotkeyModifier::Ctrl,
                HotkeyModifier::Ctrl,
            ],
            key: HotkeyKey::Space,
        };
        assert_eq!(format_chord(&chord), "Ctrl+Shift+Space");
    }
}
