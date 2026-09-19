use {
    super::CellStyle,
    serde::{
        Deserialize,
        Serialize,
    },
};

/// A cell of a [`Frame`](super::Frame), holding a grapheme
/// cluster and its style.
///
/// Wide graphemes (eg CJK ideograms or most emojis) occupy
/// two cells: a leading cell holding the cluster, and a
/// *continuation* cell which is empty and only marks the
/// second half as covered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cell {
    /// the grapheme cluster displayed in this cell.
    ///
    /// A single space for a blank cell, an empty string
    /// for the continuation cell of a wide grapheme.
    pub symbol: String,
    pub style: CellStyle,
    /// whether this cell is the second half of a wide grapheme
    pub continuation: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            symbol: " ".to_string(),
            style: CellStyle::default(),
            continuation: false,
        }
    }
}

impl Cell {
    pub fn blank(style: CellStyle) -> Self {
        Self {
            symbol: " ".to_string(),
            style,
            continuation: false,
        }
    }
    pub fn continuation(style: CellStyle) -> Self {
        Self {
            symbol: String::new(),
            style,
            continuation: true,
        }
    }
    /// whether this cell displays a blank (a space) with
    /// the default style
    pub fn is_default_blank(&self) -> bool {
        !self.continuation && self.symbol == " " && self.style.is_default()
    }
}
