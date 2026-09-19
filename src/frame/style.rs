use {
    crate::crossterm::style::{
        Attribute,
        Color,
    },
    serde::{
        Deserialize,
        Serialize,
    },
};

/// attribute bit: bold
pub const ATTR_BOLD: u16 = 1;
/// attribute bit: dim
pub const ATTR_DIM: u16 = 1 << 1;
/// attribute bit: italic
pub const ATTR_ITALIC: u16 = 1 << 2;
/// attribute bit: underlined
pub const ATTR_UNDERLINED: u16 = 1 << 3;
/// attribute bit: reverse video
pub const ATTR_REVERSE: u16 = 1 << 4;
/// attribute bit: crossed out
pub const ATTR_CROSSED_OUT: u16 = 1 << 5;
/// attribute bit: blinking
pub const ATTR_BLINK: u16 = 1 << 6;
/// attribute bit: hidden
pub const ATTR_HIDDEN: u16 = 1 << 7;

const KNOWN_ATTRIBUTES: &[(Attribute, u16)] = &[
    (Attribute::Bold, ATTR_BOLD),
    (Attribute::Dim, ATTR_DIM),
    (Attribute::Italic, ATTR_ITALIC),
    (Attribute::Underlined, ATTR_UNDERLINED),
    (Attribute::Reverse, ATTR_REVERSE),
    (Attribute::CrossedOut, ATTR_CROSSED_OUT),
    (Attribute::SlowBlink, ATTR_BLINK),
    (Attribute::RapidBlink, ATTR_BLINK),
    (Attribute::Hidden, ATTR_HIDDEN),
];

/// A terminal color, in a serializable form mirroring
/// the crossterm `Color` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FrameColor {
    Black,
    DarkGrey,
    Red,
    DarkRed,
    Green,
    DarkGreen,
    Yellow,
    DarkYellow,
    Blue,
    DarkBlue,
    Magenta,
    DarkMagenta,
    Cyan,
    DarkCyan,
    White,
    Grey,
    Rgb(u8, u8, u8),
    AnsiValue(u8),
}

impl From<Color> for FrameColor {
    fn from(color: Color) -> Self {
        match color {
            Color::Black => FrameColor::Black,
            Color::DarkGrey => FrameColor::DarkGrey,
            Color::Red => FrameColor::Red,
            Color::DarkRed => FrameColor::DarkRed,
            Color::Green => FrameColor::Green,
            Color::DarkGreen => FrameColor::DarkGreen,
            Color::Yellow => FrameColor::Yellow,
            Color::DarkYellow => FrameColor::DarkYellow,
            Color::Blue => FrameColor::Blue,
            Color::DarkBlue => FrameColor::DarkBlue,
            Color::Magenta => FrameColor::Magenta,
            Color::DarkMagenta => FrameColor::DarkMagenta,
            Color::Cyan => FrameColor::Cyan,
            Color::DarkCyan => FrameColor::DarkCyan,
            Color::White => FrameColor::White,
            Color::Grey => FrameColor::Grey,
            Color::Rgb { r, g, b } => FrameColor::Rgb(r, g, b),
            Color::AnsiValue(v) => FrameColor::AnsiValue(v),
            // Reset and unknown variants are mapped to the
            // closest stable color
            _ => FrameColor::White,
        }
    }
}

impl From<FrameColor> for Color {
    fn from(color: FrameColor) -> Self {
        match color {
            FrameColor::Black => Color::Black,
            FrameColor::DarkGrey => Color::DarkGrey,
            FrameColor::Red => Color::Red,
            FrameColor::DarkRed => Color::DarkRed,
            FrameColor::Green => Color::Green,
            FrameColor::DarkGreen => Color::DarkGreen,
            FrameColor::Yellow => Color::Yellow,
            FrameColor::DarkYellow => Color::DarkYellow,
            FrameColor::Blue => Color::Blue,
            FrameColor::DarkBlue => Color::DarkBlue,
            FrameColor::Magenta => Color::Magenta,
            FrameColor::DarkMagenta => Color::DarkMagenta,
            FrameColor::Cyan => Color::Cyan,
            FrameColor::DarkCyan => Color::DarkCyan,
            FrameColor::White => Color::White,
            FrameColor::Grey => Color::Grey,
            FrameColor::Rgb(r, g, b) => Color::Rgb { r, g, b },
            FrameColor::AnsiValue(v) => Color::AnsiValue(v),
        }
    }
}

/// The style of a frame cell: foreground, background and
/// attributes, in a serializable and comparable form.
///
/// The default style (no color, no attribute) matches the
/// state of a terminal just after a SGR reset.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CellStyle {
    pub fg: Option<FrameColor>,
    pub bg: Option<FrameColor>,
    pub attrs: u16,
}

impl CellStyle {
    pub const fn is_default(&self) -> bool {
        self.fg.is_none() && self.bg.is_none() && self.attrs == 0
    }
    pub fn has(&self, bit: u16) -> bool {
        self.attrs & bit != 0
    }
    pub fn set_attr(&mut self, bit: u16, on: bool) {
        if on {
            self.attrs |= bit;
        } else {
            self.attrs &= !bit;
        }
    }
    /// Reset to the default style (SGR 0)
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    /// Apply a SGR parameter sequence (the params of a `CSI ... m`)
    pub fn apply_sgr(&mut self, params: &[u16]) {
        if params.is_empty() {
            self.reset();
            return;
        }
        let mut iter = params.iter().peekable();
        while let Some(&param) = iter.next() {
            match param {
                0 => self.reset(),
                1 => self.set_attr(ATTR_BOLD, true),
                2 => self.set_attr(ATTR_DIM, true),
                3 => self.set_attr(ATTR_ITALIC, true),
                4 => self.set_attr(ATTR_UNDERLINED, true),
                5 | 6 => self.set_attr(ATTR_BLINK, true),
                7 => self.set_attr(ATTR_REVERSE, true),
                8 => self.set_attr(ATTR_HIDDEN, true),
                9 => self.set_attr(ATTR_CROSSED_OUT, true),
                22 => {
                    self.set_attr(ATTR_BOLD, false);
                    self.set_attr(ATTR_DIM, false);
                }
                23 => self.set_attr(ATTR_ITALIC, false),
                24 => self.set_attr(ATTR_UNDERLINED, false),
                25 => self.set_attr(ATTR_BLINK, false),
                27 => self.set_attr(ATTR_REVERSE, false),
                28 => self.set_attr(ATTR_HIDDEN, false),
                29 => self.set_attr(ATTR_CROSSED_OUT, false),
                30..=37 => self.fg = Some(FrameColor::AnsiValue((param - 30) as u8)),
                38 | 48 => {
                    let is_fg = param == 38;
                    let color = match iter.next() {
                        Some(5) => iter.next().map(|&v| FrameColor::AnsiValue(v as u8)),
                        Some(2) => {
                            let r = iter.next().copied().unwrap_or(0) as u8;
                            let g = iter.next().copied().unwrap_or(0) as u8;
                            let b = iter.next().copied().unwrap_or(0) as u8;
                            Some(FrameColor::Rgb(r, g, b))
                        }
                        _ => None,
                    };
                    if let Some(color) = color {
                        if is_fg {
                            self.fg = Some(color);
                        } else {
                            self.bg = Some(color);
                        }
                    }
                }
                39 => self.fg = None,
                40..=47 => self.bg = Some(FrameColor::AnsiValue((param - 40) as u8)),
                49 => self.bg = None,
                90..=97 => self.fg = Some(FrameColor::AnsiValue((param - 90 + 8) as u8)),
                100..=107 => self.bg = Some(FrameColor::AnsiValue((param - 100 + 8) as u8)),
                _ => {}
            }
        }
    }
    /// The crossterm attributes matching the set bits
    pub fn attributes(&self) -> Vec<Attribute> {
        KNOWN_ATTRIBUTES
            .iter()
            .filter(|(_, bit)| self.has(*bit))
            .map(|(attr, _)| *attr)
            .collect()
    }
}
