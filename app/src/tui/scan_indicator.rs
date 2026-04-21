use unicode_width::UnicodeWidthStr;

use crate::config::ScanDepth;

const INDICATOR_RECURSIVE: &str = ">>>";
const INDICATOR_ROOT_ONLY: &str = ">|⋮";
const INDICATOR_ONE_LEVEL: &str = ">⋮|";

// Must stay stable to avoid column jitter in folder lists.
const INDICATOR_TARGET_WIDTH: usize = 3;

pub fn scan_mode_indicator(depth: ScanDepth) -> &'static str {
    match depth {
        ScanDepth::RootOnly => INDICATOR_ROOT_ONLY,
        ScanDepth::OneLevel => INDICATOR_ONE_LEVEL,
        ScanDepth::Recursive => INDICATOR_RECURSIVE,
    }
}

fn fixed_width_indicator(sym: &str) -> String {
    let w = UnicodeWidthStr::width(sym);
    // Policy: if a future symbol is wider than the target width, keep it unchanged.
    // This preserves intent and avoids panics in debug builds.
    if w >= INDICATOR_TARGET_WIDTH {
        // Keep behavior stable (no truncation) even if a future symbol is wider than target.
        sym.to_string()
    } else {
        let pad = INDICATOR_TARGET_WIDTH - w;
        let mut out = String::with_capacity(sym.len() + pad);
        out.push_str(sym);
        out.extend(std::iter::repeat_n(' ', pad));
        out
    }
}

pub fn scan_depth_indicator_fixed(depth: ScanDepth) -> String {
    fixed_width_indicator(scan_mode_indicator(depth))
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn scan_mode_indicator_maps_root_only_to_expected_symbol() {
        assert_eq!(scan_mode_indicator(ScanDepth::RootOnly), ">|⋮");
    }

    #[test]
    fn scan_mode_indicator_maps_one_level_to_expected_symbol() {
        assert_eq!(scan_mode_indicator(ScanDepth::OneLevel), ">⋮|");
    }

    #[test]
    fn scan_mode_indicator_maps_recursive_to_expected_symbol() {
        assert_eq!(scan_mode_indicator(ScanDepth::Recursive), ">>>");
    }

    #[test]
    fn scan_mode_indicator_fixed_is_stable_display_width_three() {
        for depth in [
            ScanDepth::RootOnly,
            ScanDepth::OneLevel,
            ScanDepth::Recursive,
        ] {
            let fixed = scan_depth_indicator_fixed(depth);
            assert_eq!(
                UnicodeWidthStr::width(fixed.as_str()),
                3,
                "indicator must be fixed display-width=3 (depth={depth:?})"
            );
        }
    }

    #[test]
    fn scan_mode_indicator_fixed_starts_with_raw_symbol_and_only_adds_spaces() {
        for depth in [
            ScanDepth::RootOnly,
            ScanDepth::OneLevel,
            ScanDepth::Recursive,
        ] {
            let raw = scan_mode_indicator(depth);
            let fixed = scan_depth_indicator_fixed(depth);
            assert!(
                fixed.starts_with(raw),
                "fixed indicator must start with raw symbol (depth={depth:?})"
            );
            let suffix = &fixed[raw.len()..];
            assert!(
                suffix.chars().all(|c| c == ' '),
                "fixed indicator may only add spaces (depth={depth:?})"
            );
        }
    }

    #[test]
    fn fixed_width_indicator_does_not_truncate_when_wider_than_target() {
        // Policy: if a future symbol is wider than the target width, keep it unchanged.
        // This preserves intent and avoids panics in release builds.
        let sym = ">>>>";
        let out = fixed_width_indicator(sym);
        assert_eq!(out, sym);
        assert!(
            UnicodeWidthStr::width(out.as_str()) > INDICATOR_TARGET_WIDTH,
            "sanity: test symbol should be wider than target"
        );
    }
}
