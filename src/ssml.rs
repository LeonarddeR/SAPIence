//! SPVTEXTFRAG → SSML conversion with prosody mapping and mark insertion.

use std::sync::atomic::{AtomicU64, Ordering};

/// Monotonic per-utterance counter.
static UTT_COUNTER: AtomicU64 = AtomicU64::new(1);

pub fn next_utterance_id() -> u64 {
    UTT_COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Prosody adjustments from an SPVSTATE.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Prosody {
    pub rate_adj: i32,
    pub volume: u32,
    pub pitch_adj: i32,
}

impl Default for Prosody {
    fn default() -> Self {
        Self::defaults()
    }
}

impl Prosody {
    /// Default per SAPI: RateAdj 0, Volume 100, PitchAdj 0.
    pub fn defaults() -> Self {
        Self {
            rate_adj: 0,
            volume: 100,
            pitch_adj: 0,
        }
    }

    pub fn is_default(&self) -> bool {
        *self == Self::defaults()
    }
}

/// Convert a SAPI RateAdj into an SSML `rate="N%"` value.
/// Mapping: 100 * 1.1^rate_adj, rounded, minimum 1.
pub fn rate_percent(rate_adj: i32) -> u32 {
    let factor = 1.1_f64.powi(rate_adj);
    (100.0 * factor).round().max(1.0) as u32
}

/// Convert a SAPI PitchAdj into an SSML `pitch="+Nst"`/`"-Nst"` value.
pub fn pitch_attr(pitch_adj: i32) -> String {
    if pitch_adj >= 0 {
        format!("+{pitch_adj}st")
    } else {
        format!("{pitch_adj}st")
    }
}

/// Render a `<prosody>` opening tag, or `None` if all values are default.
pub fn prosody_open(p: Prosody) -> Option<String> {
    if p.is_default() {
        return None;
    }
    let mut s = String::from("<prosody");
    s.push_str(&format!(r#" rate="{}%""#, rate_percent(p.rate_adj)));
    s.push_str(&format!(r#" volume="{}""#, p.volume));
    s.push_str(&format!(r#" pitch="{}""#, pitch_attr(p.pitch_adj)));
    s.push('>');
    Some(s)
}

pub fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// Mark name for a word boundary: `w_<utt>_<word_idx>`.
pub fn word_mark(utt: u64, word_idx: u32) -> String {
    format!("w_{utt}_{word_idx}")
}
/// Mark name for utterance end: `end_<utt>`.
pub fn end_mark(utt: u64) -> String {
    format!("end_{utt}")
}
/// Mark name for a user bookmark: `bm_<utt>_<userbookmark>`.
pub fn bookmark_mark(utt: u64, name: &str) -> String {
    format!("bm_{utt}_{name}")
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParsedMark {
    Word { utt: u64, idx: u32 },
    End { utt: u64 },
    Bookmark { utt: u64, name: String },
}

pub fn parse_mark(name: &str) -> Option<ParsedMark> {
    if let Some(rest) = name.strip_prefix("w_") {
        let mut parts = rest.splitn(2, '_');
        let utt: u64 = parts.next()?.parse().ok()?;
        let idx: u32 = parts.next()?.parse().ok()?;
        return Some(ParsedMark::Word { utt, idx });
    }
    if let Some(rest) = name.strip_prefix("end_") {
        let utt: u64 = rest.parse().ok()?;
        return Some(ParsedMark::End { utt });
    }
    if let Some(rest) = name.strip_prefix("bm_") {
        let mut parts = rest.splitn(2, '_');
        let utt: u64 = parts.next()?.parse().ok()?;
        let name = parts.next()?.to_string();
        return Some(ParsedMark::Bookmark { utt, name });
    }
    None
}

/// Build SSML for one utterance. Splits text on whitespace, inserts a
/// `<mark>` after every word. Returns `(ssml_string, word_count)`.
pub fn build_utterance_ssml(
    utt: u64,
    text: &str,
    lang: &str,
    prosody: Prosody,
    bookmarks_before: &[&str],
) -> (String, u32) {
    let mut s = format!(r#"<speak version="1.0" xml:lang="{}">"#, xml_escape(lang));
    for b in bookmarks_before {
        s.push_str(&format!(
            r#"<mark name="{}"/>"#,
            xml_escape(&bookmark_mark(utt, b))
        ));
    }
    let prosody_open_tag = prosody_open(prosody);
    if let Some(open) = &prosody_open_tag {
        s.push_str(open);
    }
    let mut word_idx: u32 = 0;
    let mut wrote_any_word = false;
    for word in text.split_whitespace() {
        if wrote_any_word {
            s.push(' ');
        }
        s.push_str(&xml_escape(word));
        s.push_str(&format!(r#"<mark name="{}"/>"#, word_mark(utt, word_idx)));
        word_idx += 1;
        wrote_any_word = true;
    }
    if prosody_open_tag.is_some() {
        s.push_str("</prosody>");
    }
    s.push_str(&format!(r#"<mark name="{}"/>"#, end_mark(utt)));
    s.push_str("</speak>");
    (s, word_idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_percent_table() {
        assert_eq!(rate_percent(0), 100);
        assert_eq!(rate_percent(1), 110);
        assert_eq!(rate_percent(10), 259);
        assert_eq!(rate_percent(-1), 91);
        assert_eq!(rate_percent(-10), 39);
    }

    #[test]
    fn pitch_attr_signs() {
        assert_eq!(pitch_attr(0), "+0st");
        assert_eq!(pitch_attr(3), "+3st");
        assert_eq!(pitch_attr(-2), "-2st");
    }

    #[test]
    fn default_prosody_emits_no_tag() {
        assert!(prosody_open(Prosody::defaults()).is_none());
    }

    #[test]
    fn nondefault_prosody_emits_all_three_attrs() {
        let s = prosody_open(Prosody {
            rate_adj: 1,
            volume: 80,
            pitch_adj: -2,
        })
        .unwrap();
        assert!(s.contains(r#"rate="110%""#));
        assert!(s.contains(r#"volume="80""#));
        assert!(s.contains(r#"pitch="-2st""#));
    }

    #[test]
    fn xml_escape_basics() {
        assert_eq!(xml_escape(r#"<a&b>"x'y"#), "&lt;a&amp;b&gt;&quot;x&apos;y");
    }

    #[test]
    fn build_utterance_with_defaults() {
        let (s, n) = build_utterance_ssml(42, "hello world", "en-US", Prosody::defaults(), &[]);
        assert_eq!(n, 2);
        assert!(s.contains(r#"xml:lang="en-US""#));
        assert!(s.contains(r#"<mark name="w_42_0"/>"#));
        assert!(s.contains(r#"<mark name="w_42_1"/>"#));
        assert!(s.contains(r#"<mark name="end_42"/>"#));
        assert!(!s.contains("<prosody"));
    }

    #[test]
    fn build_utterance_with_prosody_and_bookmarks() {
        let (s, n) = build_utterance_ssml(
            7,
            "hi",
            "nl-NL",
            Prosody {
                rate_adj: -1,
                volume: 50,
                pitch_adj: 0,
            },
            &["chapter1"],
        );
        assert_eq!(n, 1);
        assert!(s.contains(r#"<mark name="bm_7_chapter1"/>"#));
        assert!(s.contains(r#"<prosody"#));
        assert!(s.contains(r#"</prosody>"#));
    }

    #[test]
    fn parse_mark_roundtrip() {
        assert_eq!(
            parse_mark("w_3_5"),
            Some(ParsedMark::Word { utt: 3, idx: 5 })
        );
        assert_eq!(parse_mark("end_9"), Some(ParsedMark::End { utt: 9 }));
        assert_eq!(
            parse_mark("bm_2_x"),
            Some(ParsedMark::Bookmark {
                utt: 2,
                name: "x".into()
            })
        );
        assert!(parse_mark("garbage").is_none());
    }
}
