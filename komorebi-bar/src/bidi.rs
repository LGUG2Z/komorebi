//! Right-to-left text support for the bar's labels.
//!
//! egui has no bidirectional text layout and no OpenType shaping yet
//! (<https://github.com/emilk/egui/issues/56>), and lays every glyph out left to
//! right in logical order. Text in a right-to-left script is therefore drawn
//! backwards, and Arabic letters are never joined, because joining is done by
//! the `init`/`medi`/`fina` substitutions of the font, which egui never runs.
//!
//! Until that is handled upstream, labels are converted before they are handed
//! to egui:
//!
//! 1. Arabic letters are replaced with the presentation forms that match their
//!    neighbours, which is what makes the letters join, and lam-alef pairs are
//!    replaced by their ligature.
//! 2. The string is reordered from logical to visual order following the
//!    Unicode Bidirectional Algorithm, so that a right-to-left run reads from
//!    right to left once egui has placed it from left to right.
//!
//! Text without any right-to-left characters is returned untouched.

use std::borrow::Cow;

use unicode_bidi::BidiInfo;
use unicode_bidi::Level;

/// Zero width joiner: asks that the letters around it be joined.
const ZWJ: char = '\u{200D}';
/// Zero width non-joiner: asks that the letters around it not be joined.
const ZWNJ: char = '\u{200C}';
/// Tatweel is drawn like a letter but only stretches the join between letters.
const TATWEEL: char = '\u{0640}';
const LAM: char = '\u{0644}';

/// Returns `text` ready to be drawn by egui: Arabic letters replaced by their
/// contextual presentation forms, and right-to-left runs in visual order.
pub fn for_display(text: &str) -> Cow<'_, str> {
    if !contains_right_to_left(text) {
        return Cow::Borrowed(text);
    }

    Cow::Owned(visual_order(&shape_arabic(text)))
}

/// Whether `text` contains a character from a script written right to left.
fn contains_right_to_left(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(
            c as u32,
            0x0590..=0x05FF      // hebrew
            | 0x0600..=0x06FF    // arabic
            | 0x0700..=0x074F    // syriac
            | 0x0750..=0x077F    // arabic supplement
            | 0x0780..=0x07BF    // thaana
            | 0x07C0..=0x08FF    // nko, samaritan, mandaic, arabic extended-a
            | 0xFB1D..=0xFDFF    // hebrew and arabic presentation forms-a
            | 0xFE70..=0xFEFF    // arabic presentation forms-b
        )
    })
}

/// Contextual forms of the Arabic letters: isolated, final, initial and medial.
/// `'\0'` marks a form the letter does not have, which is also what decides
/// whether it can join to the preceding or the following letter.
///
/// Generated from the `<isolated>`/`<final>`/`<initial>`/`<medial>` decomposition
/// mappings of Arabic Presentation Forms-A and -B in `UnicodeData.txt`. Besides
/// the Arabic letters themselves this covers the Persian, Urdu, Pashto and
/// Uyghur additions that have presentation forms.
fn forms(c: char) -> Option<(char, char, char, char)> {
    let forms = match c {
        '\u{0621}' => ('\u{FE80}', '\0', '\0', '\0'),
        '\u{0622}' => ('\u{FE81}', '\u{FE82}', '\0', '\0'),
        '\u{0623}' => ('\u{FE83}', '\u{FE84}', '\0', '\0'),
        '\u{0624}' => ('\u{FE85}', '\u{FE86}', '\0', '\0'),
        '\u{0625}' => ('\u{FE87}', '\u{FE88}', '\0', '\0'),
        '\u{0626}' => ('\u{FE89}', '\u{FE8A}', '\u{FE8B}', '\u{FE8C}'),
        '\u{0627}' => ('\u{FE8D}', '\u{FE8E}', '\0', '\0'),
        '\u{0628}' => ('\u{FE8F}', '\u{FE90}', '\u{FE91}', '\u{FE92}'),
        '\u{0629}' => ('\u{FE93}', '\u{FE94}', '\0', '\0'),
        '\u{062A}' => ('\u{FE95}', '\u{FE96}', '\u{FE97}', '\u{FE98}'),
        '\u{062B}' => ('\u{FE99}', '\u{FE9A}', '\u{FE9B}', '\u{FE9C}'),
        '\u{062C}' => ('\u{FE9D}', '\u{FE9E}', '\u{FE9F}', '\u{FEA0}'),
        '\u{062D}' => ('\u{FEA1}', '\u{FEA2}', '\u{FEA3}', '\u{FEA4}'),
        '\u{062E}' => ('\u{FEA5}', '\u{FEA6}', '\u{FEA7}', '\u{FEA8}'),
        '\u{062F}' => ('\u{FEA9}', '\u{FEAA}', '\0', '\0'),
        '\u{0630}' => ('\u{FEAB}', '\u{FEAC}', '\0', '\0'),
        '\u{0631}' => ('\u{FEAD}', '\u{FEAE}', '\0', '\0'),
        '\u{0632}' => ('\u{FEAF}', '\u{FEB0}', '\0', '\0'),
        '\u{0633}' => ('\u{FEB1}', '\u{FEB2}', '\u{FEB3}', '\u{FEB4}'),
        '\u{0634}' => ('\u{FEB5}', '\u{FEB6}', '\u{FEB7}', '\u{FEB8}'),
        '\u{0635}' => ('\u{FEB9}', '\u{FEBA}', '\u{FEBB}', '\u{FEBC}'),
        '\u{0636}' => ('\u{FEBD}', '\u{FEBE}', '\u{FEBF}', '\u{FEC0}'),
        '\u{0637}' => ('\u{FEC1}', '\u{FEC2}', '\u{FEC3}', '\u{FEC4}'),
        '\u{0638}' => ('\u{FEC5}', '\u{FEC6}', '\u{FEC7}', '\u{FEC8}'),
        '\u{0639}' => ('\u{FEC9}', '\u{FECA}', '\u{FECB}', '\u{FECC}'),
        '\u{063A}' => ('\u{FECD}', '\u{FECE}', '\u{FECF}', '\u{FED0}'),
        '\u{0641}' => ('\u{FED1}', '\u{FED2}', '\u{FED3}', '\u{FED4}'),
        '\u{0642}' => ('\u{FED5}', '\u{FED6}', '\u{FED7}', '\u{FED8}'),
        '\u{0643}' => ('\u{FED9}', '\u{FEDA}', '\u{FEDB}', '\u{FEDC}'),
        '\u{0644}' => ('\u{FEDD}', '\u{FEDE}', '\u{FEDF}', '\u{FEE0}'),
        '\u{0645}' => ('\u{FEE1}', '\u{FEE2}', '\u{FEE3}', '\u{FEE4}'),
        '\u{0646}' => ('\u{FEE5}', '\u{FEE6}', '\u{FEE7}', '\u{FEE8}'),
        '\u{0647}' => ('\u{FEE9}', '\u{FEEA}', '\u{FEEB}', '\u{FEEC}'),
        '\u{0648}' => ('\u{FEED}', '\u{FEEE}', '\0', '\0'),
        '\u{0649}' => ('\u{FEEF}', '\u{FEF0}', '\u{FBE8}', '\u{FBE9}'),
        '\u{064A}' => ('\u{FEF1}', '\u{FEF2}', '\u{FEF3}', '\u{FEF4}'),
        '\u{0671}' => ('\u{FB50}', '\u{FB51}', '\0', '\0'),
        '\u{0677}' => ('\u{FBDD}', '\0', '\0', '\0'),
        '\u{0679}' => ('\u{FB66}', '\u{FB67}', '\u{FB68}', '\u{FB69}'),
        '\u{067A}' => ('\u{FB5E}', '\u{FB5F}', '\u{FB60}', '\u{FB61}'),
        '\u{067B}' => ('\u{FB52}', '\u{FB53}', '\u{FB54}', '\u{FB55}'),
        '\u{067E}' => ('\u{FB56}', '\u{FB57}', '\u{FB58}', '\u{FB59}'),
        '\u{067F}' => ('\u{FB62}', '\u{FB63}', '\u{FB64}', '\u{FB65}'),
        '\u{0680}' => ('\u{FB5A}', '\u{FB5B}', '\u{FB5C}', '\u{FB5D}'),
        '\u{0683}' => ('\u{FB76}', '\u{FB77}', '\u{FB78}', '\u{FB79}'),
        '\u{0684}' => ('\u{FB72}', '\u{FB73}', '\u{FB74}', '\u{FB75}'),
        '\u{0686}' => ('\u{FB7A}', '\u{FB7B}', '\u{FB7C}', '\u{FB7D}'),
        '\u{0687}' => ('\u{FB7E}', '\u{FB7F}', '\u{FB80}', '\u{FB81}'),
        '\u{0688}' => ('\u{FB88}', '\u{FB89}', '\0', '\0'),
        '\u{068C}' => ('\u{FB84}', '\u{FB85}', '\0', '\0'),
        '\u{068D}' => ('\u{FB82}', '\u{FB83}', '\0', '\0'),
        '\u{068E}' => ('\u{FB86}', '\u{FB87}', '\0', '\0'),
        '\u{0691}' => ('\u{FB8C}', '\u{FB8D}', '\0', '\0'),
        '\u{0698}' => ('\u{FB8A}', '\u{FB8B}', '\0', '\0'),
        '\u{06A4}' => ('\u{FB6A}', '\u{FB6B}', '\u{FB6C}', '\u{FB6D}'),
        '\u{06A6}' => ('\u{FB6E}', '\u{FB6F}', '\u{FB70}', '\u{FB71}'),
        '\u{06A9}' => ('\u{FB8E}', '\u{FB8F}', '\u{FB90}', '\u{FB91}'),
        '\u{06AD}' => ('\u{FBD3}', '\u{FBD4}', '\u{FBD5}', '\u{FBD6}'),
        '\u{06AF}' => ('\u{FB92}', '\u{FB93}', '\u{FB94}', '\u{FB95}'),
        '\u{06B1}' => ('\u{FB9A}', '\u{FB9B}', '\u{FB9C}', '\u{FB9D}'),
        '\u{06B3}' => ('\u{FB96}', '\u{FB97}', '\u{FB98}', '\u{FB99}'),
        '\u{06BA}' => ('\u{FB9E}', '\u{FB9F}', '\0', '\0'),
        '\u{06BB}' => ('\u{FBA0}', '\u{FBA1}', '\u{FBA2}', '\u{FBA3}'),
        '\u{06BE}' => ('\u{FBAA}', '\u{FBAB}', '\u{FBAC}', '\u{FBAD}'),
        '\u{06C0}' => ('\u{FBA4}', '\u{FBA5}', '\0', '\0'),
        '\u{06C1}' => ('\u{FBA6}', '\u{FBA7}', '\u{FBA8}', '\u{FBA9}'),
        '\u{06C5}' => ('\u{FBE0}', '\u{FBE1}', '\0', '\0'),
        '\u{06C6}' => ('\u{FBD9}', '\u{FBDA}', '\0', '\0'),
        '\u{06C7}' => ('\u{FBD7}', '\u{FBD8}', '\0', '\0'),
        '\u{06C8}' => ('\u{FBDB}', '\u{FBDC}', '\0', '\0'),
        '\u{06C9}' => ('\u{FBE2}', '\u{FBE3}', '\0', '\0'),
        '\u{06CB}' => ('\u{FBDE}', '\u{FBDF}', '\0', '\0'),
        '\u{06CC}' => ('\u{FBFC}', '\u{FBFD}', '\u{FBFE}', '\u{FBFF}'),
        '\u{06D0}' => ('\u{FBE4}', '\u{FBE5}', '\u{FBE6}', '\u{FBE7}'),
        '\u{06D2}' => ('\u{FBAE}', '\u{FBAF}', '\0', '\0'),
        '\u{06D3}' => ('\u{FBB0}', '\u{FBB1}', '\0', '\0'),
        _ => return None,
    };

    Some(forms)
}

/// Whether `c` can join to the letter before it.
fn joins_previous(c: char) -> bool {
    forms(c).is_some_and(|forms| forms.1 != '\0')
}

/// Whether `c` can join to the letter after it.
fn joins_next(c: char) -> bool {
    forms(c).is_some_and(|forms| forms.2 != '\0')
}

/// Whether `c` is a mark that is drawn on the character before it and does not
/// interrupt the join between letters.
fn is_mark(c: char) -> bool {
    matches!(
        c as u32,
        0x0300..=0x036F      // combining diacritical marks
        | 0x0483..=0x0489
        | 0x0591..=0x05C7    // hebrew points
        | 0x0610..=0x061A
        | 0x064B..=0x065F    // arabic harakat
        | 0x0670
        | 0x06D6..=0x06ED
        | 0x08D3..=0x08FF
        | 0x0E31 | 0x0E34..=0x0E3A | 0x0E47..=0x0E4E // thai marks
    )
}

/// The ligature of lam followed by `alef`, as (isolated, final).
fn lam_alef_ligature(alef: char) -> Option<(char, char)> {
    match alef {
        '\u{0622}' => Some(('\u{FEF5}', '\u{FEF6}')),
        '\u{0623}' => Some(('\u{FEF7}', '\u{FEF8}')),
        '\u{0625}' => Some(('\u{FEF9}', '\u{FEFA}')),
        '\u{0627}' => Some(('\u{FEFB}', '\u{FEFC}')),
        _ => None,
    }
}

/// Replaces every Arabic letter in `text` with the presentation form that
/// matches its neighbours.
fn shape_arabic(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());

    // Whether the previous letter is able to join to the one being written.
    let mut previous_joins = false;
    let mut index = 0;

    while index < chars.len() {
        let c = chars[index];

        if c == ZWJ || c == ZWNJ {
            // Join controls only affect the choice of form; they are not drawn.
            index += 1;
            continue;
        }

        if c == TATWEEL {
            // Stretches the join without changing it.
            out.push(c);
            index += 1;
            continue;
        }

        if is_mark(c) {
            out.push(c);
            index += 1;
            continue;
        }

        let (next_index, next, next_zwj) = next_joining(&chars, index + 1);
        let next_joins = next.is_some_and(joins_previous) || next_zwj;

        if c == LAM
            && next_joins
            && let Some(ligature) = next.and_then(lam_alef_ligature)
        {
            out.push(if previous_joins {
                ligature.1
            } else {
                ligature.0
            });
            index = next_index.expect("lam is followed by an alef") + 1;
            previous_joins = false;
            continue;
        }

        let Some((isolated, final_form, initial, medial)) = forms(c) else {
            out.push(c);
            previous_joins = false;
            index += 1;
            continue;
        };

        let joins_prev = previous_joins && final_form != '\0';
        let joins_following = next_joins && initial != '\0';

        out.push(match (joins_prev, joins_following) {
            (true, true) => medial,
            (true, false) => final_form,
            (false, true) => initial,
            (false, false) => isolated,
        });

        previous_joins = joins_next(c);
        index += 1;
    }

    out
}

/// Returns the index and value of the first character after `from` that takes
/// part in joining, and whether a zero width joiner was seen on the way.
fn next_joining(chars: &[char], from: usize) -> (Option<usize>, Option<char>, bool) {
    let mut zwj = false;

    for (offset, &c) in chars.iter().enumerate().skip(from) {
        if c == ZWJ {
            zwj = true;
            continue;
        }

        if is_mark(c) || c == TATWEEL {
            continue;
        }

        return (Some(offset), Some(c), zwj);
    }

    (None, None, zwj)
}

/// Reorders every line of `text` into the order the glyphs are drawn in.
fn visual_order(text: &str) -> String {
    let mut out = String::with_capacity(text.len());

    for line in text.split_inclusive('\n') {
        let (body, newline) = match line.strip_suffix('\n') {
            Some(body) => (body, "\n"),
            None => (line, ""),
        };

        out.push_str(&visual_order_line(body));
        out.push_str(newline);
    }

    out
}

/// Reorders a single line into visual order, following rule L2 of the Unicode
/// Bidirectional Algorithm. The bar is a left-to-right container, so the
/// paragraph level is always left to right.
fn visual_order_line(line: &str) -> String {
    let bidi = BidiInfo::new(line, Some(Level::ltr()));
    let mut out = String::with_capacity(line.len());

    for paragraph in &bidi.paragraphs {
        // A mark is drawn on the character it follows, so the two are kept
        // together while runs are reversed.
        let mut starts: Vec<usize> = Vec::new();
        for (offset, c) in line[paragraph.range.clone()].char_indices() {
            if is_mark(c) && !starts.is_empty() {
                continue;
            }
            starts.push(paragraph.range.start + offset);
        }

        let mut clusters: Vec<(&str, Level)> = Vec::with_capacity(starts.len());
        for (i, &start) in starts.iter().enumerate() {
            let end = starts.get(i + 1).copied().unwrap_or(paragraph.range.end);
            clusters.push((&line[start..end], bidi.levels[start]));
        }

        let highest = clusters
            .iter()
            .map(|(_, level)| level.number())
            .max()
            .unwrap_or(0);
        let lowest_odd = clusters
            .iter()
            .map(|(_, level)| level.number())
            .filter(|level| level % 2 == 1)
            .min();

        if let Some(lowest_odd) = lowest_odd {
            for level in (lowest_odd..=highest).rev() {
                reverse_runs(&mut clusters, level);
            }
        }

        for (cluster, _) in clusters {
            out.push_str(cluster);
        }
    }

    out
}

/// Reverses every run of clusters whose embedding level is `level` or higher.
fn reverse_runs(clusters: &mut [(&str, Level)], level: u8) {
    let mut start = 0;

    while start < clusters.len() {
        if clusters[start].1.number() < level {
            start += 1;
            continue;
        }

        let mut end = start + 1;
        while end < clusters.len() && clusters[end].1.number() >= level {
            end += 1;
        }

        clusters[start..end].reverse();
        start = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin_text_is_left_alone() {
        assert!(matches!(for_display("hello"), Cow::Borrowed(_)));
        assert_eq!(for_display("1 - focused"), "1 - focused");
    }

    #[test]
    fn arabic_letters_are_joined() {
        // م ر ح ب ا: initial, final, initial, medial, final
        assert_eq!(
            shape_arabic("مرحبا"),
            "\u{FEE3}\u{FEAE}\u{FEA3}\u{FE92}\u{FE8E}"
        );

        // س ل ا م: the lam-alef pair becomes its final ligature
        assert_eq!(shape_arabic("سلام"), "\u{FEB3}\u{FEFC}\u{FEE1}");
    }

    #[test]
    fn lam_alef_pair_becomes_a_ligature() {
        assert_eq!(shape_arabic("لا"), "\u{FEFB}");
        assert_eq!(shape_arabic("أهلا"), "\u{FE83}\u{FEEB}\u{FEFC}");
    }

    #[test]
    fn arabic_is_drawn_right_to_left() {
        // A paragraph of only right-to-left text is reversed as a whole.
        assert_eq!(
            for_display("مرحبا"),
            "\u{FE8E}\u{FE92}\u{FEA3}\u{FEAE}\u{FEE3}"
        );
    }

    #[test]
    fn hebrew_is_reordered_without_shaping() {
        assert_eq!(for_display("שלום"), "םולש");
    }

    #[test]
    fn left_to_right_text_keeps_its_run_in_place() {
        let displayed = for_display("Tariq - مرحبا");

        assert!(displayed.starts_with("Tariq - "));
        assert_eq!(
            &displayed["Tariq - ".len()..],
            "\u{FE8E}\u{FE92}\u{FEA3}\u{FEAE}\u{FEE3}"
        );
    }

    #[test]
    fn marks_stay_with_their_letter() {
        // The fatha of the reh is drawn on the reh: ر is isolated here because
        // it does not join to the following beh.
        assert_eq!(for_display("رَب"), "\u{FE8F}\u{064E}\u{FEAD}");
    }
}
