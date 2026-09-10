//! Official Unicode BiDi conformance test suite.
//!
//! This test runs the official Unicode `BidiTest.txt` and
//! `BidiCharacterTest.txt` conformance corpora (UCD 17.0.0)
//! against the `unicode_bidi` crate that powers
//! `martensite_text::bidi`.
//!
//! The data files are vendored under `tests/data/` and are
//! © 2025 Unicode®, Inc., licensed under the Unicode License v3
//! (see `tests/data/LICENSE-unicode`).
//!
//! These tests are `#[ignore]` by default because the full
//! `BidiTest.txt` corpus expands to ~770k cases and
//! `BidiCharacterTest.txt` has ~92k cases. Run with:
//!
//! ```sh
//! cargo test -p martensite-text --test bidi_conformance -- --ignored
//! ```
//!
//! A CI job downloads the corpora and runs these tests in full.

#![forbid(unsafe_code)]

use std::path::Path;

use unicode_bidi::{BidiClass, BidiInfo, Level};

// ===========================================================================
// BidiTest.txt — class-vector conformance
// ===========================================================================

/// Maps a BidiClass name token from BidiTest.txt to a representative
/// Unicode character with that bidi class. This is necessary because
/// `unicode_bidi::BidiInfo::new` takes actual text, not class vectors.
fn class_to_char(class: &str) -> Option<char> {
    let bc = match class {
        "AL" => BidiClass::AL,
        "AN" => BidiClass::AN,
        "B" => BidiClass::B,
        "BN" => BidiClass::BN,
        "CS" => BidiClass::CS,
        "EN" => BidiClass::EN,
        "ES" => BidiClass::ES,
        "ET" => BidiClass::ET,
        "FSI" => BidiClass::FSI,
        "L" => BidiClass::L,
        "LRE" => BidiClass::LRE,
        "LRI" => BidiClass::LRI,
        "LRO" => BidiClass::LRO,
        "NSM" => BidiClass::NSM,
        "ON" => BidiClass::ON,
        "PDF" => BidiClass::PDF,
        "PDI" => BidiClass::PDI,
        "R" => BidiClass::R,
        "RLE" => BidiClass::RLE,
        "RLI" => BidiClass::RLI,
        "RLO" => BidiClass::RLO,
        "S" => BidiClass::S,
        "WS" => BidiClass::WS,
        _ => return None,
    };

    // Representative characters for each bidi class.
    // These are chosen so that unicode_bidi::bidi_class(ch) returns
    // the expected class. Using a single representative per class is
    // sufficient because BidiTest.txt tests the algorithm at the
    // class-vector level, not the character level.
    let ch = match bc {
        BidiClass::AL => '\u{0627}',  // Arabic Letter — ARABIC LETTER ALEF
        BidiClass::AN => '\u{0660}',  // Arabic Number — ARABIC-INDIC DIGIT
        BidiClass::B => '\u{000A}',   // Paragraph Separator — LINE FEED
        BidiClass::BN => '\u{200B}',  // Boundary Neutral — ZERO WIDTH SPACE
        BidiClass::CS => '\u{002C}',  // Common Separator — COMMA
        BidiClass::EN => '\u{0030}',  // European Number — DIGIT ZERO
        BidiClass::ES => '\u{002B}',  // European Separator — PLUS SIGN
        BidiClass::ET => '\u{0025}',  // European Terminator — PERCENT SIGN
        BidiClass::FSI => '\u{2068}', // First Strong Isolate
        BidiClass::L => '\u{0041}',   // Left-to-Right — LATIN A
        BidiClass::LRE => '\u{202A}', // Left-to-Right Embedding
        BidiClass::LRI => '\u{2066}', // Left-to-Right Isolate
        BidiClass::LRO => '\u{202D}', // Left-to-Right Override
        BidiClass::NSM => '\u{0300}', // Nonspacing Mark — COMBINING GRAVE
        BidiClass::ON => '\u{0021}',  // Other Neutral — EXCLAMATION MARK
        BidiClass::PDF => '\u{202C}', // Pop Directional Formatting
        BidiClass::PDI => '\u{2069}', // Pop Directional Isolate
        BidiClass::R => '\u{05D0}',   // Right-to-Left — HEBREW ALEF
        BidiClass::RLE => '\u{202B}', // Right-to-Left Embedding
        BidiClass::RLI => '\u{2067}', // Right-to-Left Isolate
        BidiClass::RLO => '\u{202E}', // Right-to-Left Override
        BidiClass::S => '\u{0009}',   // Segment Separator — TAB
        BidiClass::WS => '\u{0020}',  // Whitespace — SPACE
    };
    Some(ch)
}

/// Parses and runs BidiTest.txt, returning (total, passed, failed).
fn run_bidi_test(path: &Path) -> (u64, u64, u64) {
    let data = std::fs::read_to_string(path).expect("BidiTest.txt");
    let mut expected_levels: Vec<Option<u8>> = Vec::new();
    let mut expected_reorder: Vec<usize> = Vec::new();
    let mut total = 0u64;
    let mut passed = 0u64;
    let mut failed = 0u64;

    for line in data.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with("@Levels:") {
            expected_levels.clear();
            for field in line.split_whitespace().skip(1) {
                if field == "x" {
                    expected_levels.push(None);
                } else {
                    expected_levels.push(Some(field.parse().unwrap()));
                }
            }
            continue;
        }

        if line.starts_with("@Reorder:") {
            expected_reorder.clear();
            for field in line.split_whitespace().skip(1) {
                expected_reorder.push(field.parse().unwrap());
            }
            continue;
        }

        if line.starts_with('@') {
            continue;
        }

        // Data line: <class> <class> ... ; <bitset>
        let parts: Vec<&str> = line.split(';').collect();
        if parts.len() != 2 {
            continue;
        }

        let class_tokens: Vec<&str> = parts[0].split_whitespace().collect();
        let bitset: u32 = parts[1].trim().parse().unwrap_or(0);

        // Build a representative string from the class vector.
        let text: String = class_tokens
            .iter()
            .filter_map(|t| class_to_char(t))
            .collect();

        if text.is_empty() {
            continue;
        }

        // Determine which positions are X9-removed (formatting characters).
        // BidiTest.txt marks these with "x" in @Levels. We use this to
        // filter them from the reorder comparison.
        let x9_mask: Vec<bool> = if expected_levels.len() == class_tokens.len() {
            expected_levels.iter().map(|l| l.is_none()).collect()
        } else {
            vec![false; text.chars().count()]
        };

        // Test each paragraph direction indicated by the bitset.
        let mut directions: Vec<Option<Level>> = Vec::new();
        if bitset & 1 != 0 {
            directions.push(None); // Auto
        }
        if bitset & 2 != 0 {
            directions.push(Some(Level::ltr())); // LTR
        }
        if bitset & 4 != 0 {
            directions.push(Some(Level::rtl())); // RTL
        }

        for default_level in &directions {
            total += 1;
            let bidi_info = BidiInfo::new(&text, *default_level);

            // Concatenate levels from all paragraphs. BidiTest.txt expects
            // levels for the entire line, but BidiInfo splits at B
            // (paragraph separator) characters. We reassemble by iterating
            // through all paragraphs in order.
            let mut all_levels: Vec<Level> = Vec::new();
            for para in &bidi_info.paragraphs {
                let para_levels = bidi_info.reordered_levels_per_char(para, para.range.clone());
                all_levels.extend(para_levels);
            }

            // Compare resolved levels. X9-removed characters (marked "x"
            // in @Levels) are accepted with any level.
            let level_match = all_levels.len() == expected_levels.len()
                && all_levels.iter().zip(&expected_levels).all(
                    |(actual, expected)| match expected {
                        None => true, // X9-removed
                        Some(exp) => actual.number() == *exp,
                    },
                );

            if level_match {
                // Check reordering. @Reorder gives the logical indices of
                // non-X9 characters in visual order. We compute the actual
                // visual order by calling reorder_visual on the concatenated
                // levels, then filtering out X9 positions.
                let full_visual = BidiInfo::reorder_visual(&all_levels);
                let actual_reorder: Vec<usize> = full_visual
                    .into_iter()
                    .filter(|&i| !x9_mask.get(i).copied().unwrap_or(false))
                    .collect();

                let reorder_match =
                    actual_reorder == expected_reorder || expected_reorder.is_empty();

                if reorder_match {
                    passed += 1;
                } else {
                    failed += 1;
                }
            } else {
                failed += 1;
            }
        }
    }

    (total, passed, failed)
}

#[test]
#[ignore = "full BidiTest.txt corpus (~770k cases); run with --ignored"]
fn bidi_test_conformance() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join("BidiTest.txt");

    if !path.exists() {
        eprintln!("BidiTest.txt not found at {path:?}; skipping (CI downloads it)");
        return;
    }

    let (total, passed, failed) = run_bidi_test(&path);
    eprintln!(
        "BidiTest.txt: {total} cases, {passed} passed, {failed} failed ({:.2}% pass rate)",
        if total > 0 {
            passed as f64 / total as f64 * 100.0
        } else {
            100.0
        }
    );

    // 100% pass rate required — the representative-character mapping
    // covers all bidi classes used by BidiTest.txt.
    let pass_rate = if total > 0 {
        passed as f64 / total as f64
    } else {
        1.0
    };
    assert!(
        pass_rate >= 1.0,
        "BidiTest.txt pass rate {pass_rate:.6} is below 100% threshold ({failed} failures out of {total})"
    );
}

// ===========================================================================
// BidiCharacterTest.txt — code-point conformance
// ===========================================================================

/// Parses and runs BidiCharacterTest.txt, returning (total, passed, failed).
fn run_bidi_character_test(path: &Path) -> (u64, u64, u64) {
    let data = std::fs::read_to_string(path).expect("BidiCharacterTest.txt");
    let mut total = 0u64;
    let mut passed = 0u64;
    let mut failed = 0u64;

    for line in data.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Fields: code-points ; direction ; para-level ; resolved-levels ; visual-order
        let fields: Vec<&str> = line.split(';').collect();
        if fields.len() < 5 {
            continue;
        }

        // Parse code points.
        let codepoints: Vec<char> = fields[0]
            .split_whitespace()
            .filter_map(|hex| u32::from_str_radix(hex, 16).ok())
            .filter_map(char::from_u32)
            .collect();

        if codepoints.is_empty() {
            continue;
        }

        let text: String = codepoints.iter().collect();

        // Parse paragraph direction.
        let direction: i32 = fields[1].trim().parse().unwrap_or(0);
        let default_level = match direction {
            1 => Some(Level::rtl()),
            2 => None, // Auto
            _ => Some(Level::ltr()),
        };

        // Parse expected resolved levels.
        let expected_levels: Vec<Option<u8>> = fields[3]
            .split_whitespace()
            .map(|f| {
                if f == "x" {
                    None
                } else {
                    Some(f.parse().unwrap_or(0))
                }
            })
            .collect();

        // Parse expected visual order.
        let expected_order: Vec<usize> = fields[4]
            .split_whitespace()
            .filter_map(|f| f.parse().ok())
            .collect();

        total += 1;

        let bidi_info = BidiInfo::new(&text, default_level);

        let para = match bidi_info.paragraphs.first() {
            Some(p) => p,
            None => {
                failed += 1;
                continue;
            }
        };

        let levels = bidi_info.reordered_levels_per_char(para, para.range.clone());

        // Compare resolved levels.
        let level_match = levels.len() == expected_levels.len()
            && levels
                .iter()
                .zip(&expected_levels)
                .all(|(actual, expected)| match expected {
                    None => true,
                    Some(exp) => actual.number() == *exp,
                });

        if level_match {
            // Also check visual order.
            // The expected order lists indices of non-X9-removed chars.
            // We compute the actual visual order by calling reorder_visual
            // on the full levels, then filtering out X9 characters (those
            // with expected level "x"/None).
            let x9_mask: Vec<bool> = expected_levels.iter().map(|l| l.is_none()).collect();
            let actual_order: Vec<usize> = BidiInfo::reorder_visual(&levels)
                .into_iter()
                .filter(|&i| !x9_mask.get(i).copied().unwrap_or(false))
                .collect();

            let order_match = expected_order.is_empty() || actual_order == expected_order;

            if order_match {
                passed += 1;
            } else {
                failed += 1;
            }
        } else {
            failed += 1;
        }
    }

    (total, passed, failed)
}

#[test]
#[ignore = "full BidiCharacterTest.txt corpus (~92k cases); run with --ignored"]
fn bidi_character_test_conformance() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join("BidiCharacterTest.txt");

    if !path.exists() {
        eprintln!("BidiCharacterTest.txt not found at {path:?}; skipping (CI downloads it)");
        return;
    }

    let (total, passed, failed) = run_bidi_character_test(&path);
    eprintln!(
        "BidiCharacterTest.txt: {total} cases, {passed} passed, {failed} failed ({:.2}% pass rate)",
        if total > 0 {
            passed as f64 / total as f64 * 100.0
        } else {
            100.0
        }
    );

    // 100% pass rate required.
    let pass_rate = if total > 0 {
        passed as f64 / total as f64
    } else {
        1.0
    };
    assert!(
        pass_rate >= 1.0,
        "BidiCharacterTest.txt pass rate {pass_rate:.6} is below 100% threshold ({failed} failures out of {total})"
    );
}

// ===========================================================================
// Smoke test (always runs, uses a tiny subset)
// ===========================================================================

#[test]
fn bidi_test_smoke() {
    // Verify the parser works on a tiny inline sample matching the real
    // BidiTest.txt format. L R with Auto+LTR (bitset 3): levels 0 1,
    // reorder 0 1 (no reordering for LTR base with single R at level 1).
    let sample = "\
# Test
@Levels: 0 1
@Reorder: 0 1
L R; 3
";
    let temp = std::env::temp_dir().join("martensite_bidi_test_smoke.txt");
    std::fs::write(&temp, sample).unwrap();

    let (total, passed, failed) = run_bidi_test(&temp);
    assert_eq!(total, 2, "should test 2 paragraph directions (bitset 3)");
    assert_eq!(failed, 0, "smoke test should have 0 failures, got {failed}");
    assert_eq!(passed, 2);

    let _ = std::fs::remove_file(&temp);
}

#[test]
fn bidi_character_test_smoke() {
    // A simple case: Latin A + Hebrew Alef in a LTR paragraph.
    // Levels: A=0, Alef=1. Visual order: A first (level 0, LTR), then
    // Alef (level 1, RTL run of length 1 — no reordering for single char).
    // So the visual order is [0, 1] — same as logical.
    let sample = "\
# Test
0041 05D0;0;0;0 1;0 1
";
    let temp = std::env::temp_dir().join("martensite_bidi_char_test_smoke.txt");
    std::fs::write(&temp, sample).unwrap();

    let (total, passed, failed) = run_bidi_character_test(&temp);
    assert_eq!(total, 1);
    assert_eq!(failed, 0, "smoke test should pass, got {failed} failures");
    assert_eq!(passed, 1);

    let _ = std::fs::remove_file(&temp);
}
