//! Parsing of the ANSI (SGR) sequences produced by crossterm's
//! `StyledContent` display, back into a sequence of styled graphemes.
//!
//! This lets the frame API capture exactly what the usual termimad
//! rendering would write, without duplicating the rendering logic.

use {
    crate::crossterm::style::{
        Attribute,
        Color,
        ContentStyle,
    },
    unicode_segmentation::UnicodeSegmentation,
};

/// Parse a string containing ANSI SGR sequences into a succession
/// of grapheme clusters, each with the style which was active when
/// it appeared.
///
/// Non SGR escape sequences are ignored (they're not produced by
/// the rendering of lines).
pub(crate) fn parse_styled_graphemes(s: &str) -> Vec<(String, ContentStyle)> {
    let mut out = Vec::new();
    let mut style = ContentStyle::default();
    let mut text = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next(); // consume '['
            let mut params = String::new();
            let mut final_byte = None;
            for c2 in chars.by_ref() {
                if ('@'..='~').contains(&c2) {
                    final_byte = Some(c2);
                    break;
                }
                params.push(c2);
            }
            flush_text(&mut text, style, &mut out);
            if final_byte == Some('m') {
                apply_sgr(&params, &mut style);
            }
        } else {
            text.push(c);
        }
    }
    flush_text(&mut text, style, &mut out);
    out
}

fn flush_text(
    text: &mut String,
    style: ContentStyle,
    out: &mut Vec<(String, ContentStyle)>,
) {
    if text.is_empty() {
        return;
    }
    for grapheme in text.graphemes(true) {
        out.push((grapheme.to_string(), style));
    }
    text.clear();
}

/// Apply a SGR sequence (the part between `CSI` and the final `m`)
/// to the given style.
fn apply_sgr(params: &str, style: &mut ContentStyle) {
    if params.is_empty() {
        *style = ContentStyle::default();
        return;
    }
    let tokens: Vec<&str> = params.split(';').collect();
    let mut i = 0;
    while i < tokens.len() {
        // sub-parameters (eg "4:3") aren't produced by crossterm for
        // the styles termimad uses: only the base parameter is read
        let base = tokens[i].split(':').next().unwrap_or("");
        match base {
            "" | "0" => *style = ContentStyle::default(),
            "1" => set_attr(style, Attribute::Bold),
            "2" => set_attr(style, Attribute::Dim),
            "3" => set_attr(style, Attribute::Italic),
            "4" => set_attr(style, Attribute::Underlined),
            "5" => set_attr(style, Attribute::SlowBlink),
            "6" => set_attr(style, Attribute::RapidBlink),
            "7" => set_attr(style, Attribute::Reverse),
            "8" => set_attr(style, Attribute::Hidden),
            "9" => set_attr(style, Attribute::CrossedOut),
            "20" => set_attr(style, Attribute::Fraktur),
            "21" => unset_attr(style, Attribute::Bold),
            "22" => {
                unset_attr(style, Attribute::Bold);
                unset_attr(style, Attribute::Dim);
            }
            "23" => unset_attr(style, Attribute::Italic),
            "24" => unset_attr(style, Attribute::Underlined),
            "25" => {
                unset_attr(style, Attribute::SlowBlink);
                unset_attr(style, Attribute::RapidBlink);
            }
            "27" => unset_attr(style, Attribute::Reverse),
            "28" => unset_attr(style, Attribute::Hidden),
            "29" => unset_attr(style, Attribute::CrossedOut),
            "51" => set_attr(style, Attribute::Framed),
            "52" => set_attr(style, Attribute::Encircled),
            "53" => set_attr(style, Attribute::OverLined),
            "54" => {
                unset_attr(style, Attribute::Framed);
                unset_attr(style, Attribute::Encircled);
            }
            "55" => unset_attr(style, Attribute::OverLined),
            "38" | "48" | "58" => {
                let (color, consumed) = parse_extended_color(&tokens[i + 1..]);
                i += consumed;
                let slot = match base {
                    "38" => &mut style.foreground_color,
                    "48" => &mut style.background_color,
                    _ => &mut style.underline_color,
                };
                *slot = color;
            }
            "39" => style.foreground_color = None,
            "49" => style.background_color = None,
            "59" => style.underline_color = None,
            _ => {} // unknown parameter: ignore
        }
        i += 1;
    }
}

/// Parse the "5;n" or "2;r;g;b" following a 38/48/58 parameter.
/// Return the color and the number of consumed tokens.
fn parse_extended_color(tokens: &[&str]) -> (Option<Color>, usize) {
    match tokens.first() {
        Some(&"5") => {
            let color = tokens
                .get(1)
                .and_then(|s| s.parse::<u8>().ok())
                .map(ansi_value_to_color);
            (color, 2)
        }
        Some(&"2") => {
            let mut channels = [0u8; 3];
            let mut ok = true;
            for (k, channel) in channels.iter_mut().enumerate() {
                match tokens.get(1 + k).and_then(|s| s.parse::<u8>().ok()) {
                    Some(v) => *channel = v,
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok {
                (
                    Some(Color::Rgb {
                        r: channels[0],
                        g: channels[1],
                        b: channels[2],
                    }),
                    4,
                )
            } else {
                (None, 1)
            }
        }
        _ => (None, 0),
    }
}

/// Map a 256-palette index to the color crossterm would have
/// emitted it for, so that a parse/emit round-trip is stable.
fn ansi_value_to_color(value: u8) -> Color {
    match value {
        0 => Color::Black,
        1 => Color::DarkRed,
        2 => Color::DarkGreen,
        3 => Color::DarkYellow,
        4 => Color::DarkBlue,
        5 => Color::DarkMagenta,
        6 => Color::DarkCyan,
        7 => Color::Grey,
        8 => Color::DarkGrey,
        9 => Color::Red,
        10 => Color::Green,
        11 => Color::Yellow,
        12 => Color::Blue,
        13 => Color::Magenta,
        14 => Color::Cyan,
        15 => Color::White,
        _ => Color::AnsiValue(value),
    }
}

fn set_attr(style: &mut ContentStyle, attr: Attribute) {
    style.attributes.set(attr);
}

fn unset_attr(style: &mut ContentStyle, attr: Attribute) {
    style.attributes.unset(attr);
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::crossterm::style::Stylize,
    };

    #[test]
    fn parse_plain_text() {
        let parsed = parse_styled_graphemes("hello");
        assert_eq!(parsed.len(), 5);
        assert!(parsed.iter().all(|(_, s)| *s == ContentStyle::default()));
    }

    #[test]
    fn parse_crossterm_styled_content() {
        // colors may be globally disabled (eg NO_COLOR env)
        crate::crossterm::style::Colored::set_ansi_color_disabled(false);
        let s = format!("{}", "ab".red().bold());
        let parsed = parse_styled_graphemes(&s);
        assert_eq!(parsed.len(), 2);
        for (g, style) in &parsed {
            assert_eq!(style.foreground_color, Some(Color::Red));
            assert!(style.attributes.has(Attribute::Bold));
            assert!(g == "a" || g == "b");
        }
        // the reset written by crossterm must bring us back to default
        let s = format!("{}\u{1b}[0mx", "a".red());
        let parsed = parse_styled_graphemes(&s);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[1].0, "x");
        assert_eq!(parsed[1].1, ContentStyle::default());
    }

    #[test]
    fn parse_rgb_and_ansi_value() {
        crate::crossterm::style::Colored::set_ansi_color_disabled(false);
        let s = format!(
            "{}",
            "x".with(Color::Rgb { r: 12, g: 34, b: 56 })
                .on(Color::AnsiValue(42))
        );
        let parsed = parse_styled_graphemes(&s);
        assert_eq!(parsed.len(), 1);
        assert_eq!(
            parsed[0].1.foreground_color,
            Some(Color::Rgb { r: 12, g: 34, b: 56 })
        );
        assert_eq!(parsed[0].1.background_color, Some(Color::AnsiValue(42)));
    }
}
