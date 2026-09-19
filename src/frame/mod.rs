//! Viewport frames: serializable snapshots of what a view renders
//! in an area, and minimal row-level patches between two frames.
//!
//! This module is designed for low-bandwidth remote terminals:
//! instead of rewriting the whole screen on every change, you keep
//! the previous [`Frame`], render the new one, and write only the
//! [`FramePatch`] between them. A frame can be serialized (it holds
//! its size, a skin fingerprint and a format version) so that a
//! client can save it and, after a reconnection, decide whether a
//! diff is still applicable or a full frame must be sent.
//!
//! No background thread nor event loop is involved: frames are
//! computed on demand, and the usual rendering methods keep
//! writing the full content by default.

mod ansi;
mod patch;

pub use patch::{
    FramePatch,
    PatchOp,
};

use {
    crate::{
        crossterm::style::{
            Attributes,
            Color,
            ContentStyle,
        },
        MadSkin,
    },
    serde::{
        Deserialize,
        Serialize,
    },
    std::collections::hash_map::DefaultHasher,
    std::hash::Hasher,
    unicode_width::UnicodeWidthStr,
};

/// Version of the frame format. Frames (or patches) built with a
/// different version are never considered compatible.
pub const FRAME_FORMAT_VERSION: u16 = 1;

/// The style of one terminal cell.
///
/// This is a serializable mirror of crossterm's `ContentStyle`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellStyle {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub underline_color: Option<Color>,
    #[serde(default, with = "attributes_serde")]
    pub attributes: Attributes,
}

/// crossterm's `Attributes` doesn't implement serde: it's
/// serialized as the list of the attributes it holds.
mod attributes_serde {
    use {
        crate::crossterm::style::{
            Attribute,
            Attributes,
        },
        serde::{
            Deserialize,
            Deserializer,
            Serialize,
            Serializer,
        },
    };

    pub fn serialize<S: Serializer>(attrs: &Attributes, serializer: S) -> Result<S::Ok, S::Error> {
        Attribute::iterator()
            .filter(|attr| attrs.has(*attr))
            .collect::<Vec<Attribute>>()
            .serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Attributes, D::Error> {
        let attrs = Vec::<Attribute>::deserialize(deserializer)?;
        Ok(Attributes::from(attrs.as_slice()))
    }
}

impl From<ContentStyle> for CellStyle {
    fn from(cs: ContentStyle) -> Self {
        Self {
            fg: cs.foreground_color,
            bg: cs.background_color,
            underline_color: cs.underline_color,
            attributes: cs.attributes,
        }
    }
}

impl From<CellStyle> for ContentStyle {
    fn from(cs: CellStyle) -> Self {
        ContentStyle {
            foreground_color: cs.fg,
            background_color: cs.bg,
            underline_color: cs.underline_color,
            attributes: cs.attributes,
        }
    }
}

/// One terminal cell of a frame.
///
/// A wide grapheme (eg a CJK char or an emoji) occupies two
/// consecutive cells: the first one holds the grapheme in `symbol`,
/// the second one is a *continuation* cell with an empty symbol.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cell {
    /// The grapheme displayed in this cell. A single space for a
    /// blank cell, an empty string for the continuation cell of a
    /// wide grapheme.
    pub symbol: String,
    pub style: CellStyle,
}

impl Default for Cell {
    fn default() -> Self {
        Self::blank()
    }
}

impl Cell {
    pub fn blank() -> Self {
        Self {
            symbol: " ".to_string(),
            style: CellStyle::default(),
        }
    }
    pub fn continuation(style: CellStyle) -> Self {
        Self {
            symbol: String::new(),
            style,
        }
    }
    /// A blank cell is an unstyled or styled space (not the
    /// continuation of a wide grapheme).
    pub fn is_blank(&self) -> bool {
        self.symbol == " "
    }
    pub fn is_continuation(&self) -> bool {
        self.symbol.is_empty()
    }
}

/// A serializable snapshot of the cells displayed by a view in
/// its area.
///
/// A frame is content-addressed: two frames built from different
/// objects but with the same content are equal, so diffs don't
/// depend on the identity of the internal line objects.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    /// Format version, see [`FRAME_FORMAT_VERSION`].
    pub version: u16,
    /// Width of the viewport, in terminal columns.
    pub width: u16,
    /// Height of the viewport, in terminal rows.
    pub height: u16,
    /// Fingerprint of the skin the frame was rendered with.
    pub skin_fingerprint: u64,
    rows: Vec<Vec<Cell>>,
}

impl Frame {
    /// Build a blank frame of the given size.
    pub fn new(width: u16, height: u16, skin_fingerprint: u64) -> Self {
        Self {
            version: FRAME_FORMAT_VERSION,
            width,
            height,
            skin_fingerprint,
            rows: vec![vec![Cell::blank(); width as usize]; height as usize],
        }
    }

    /// Total number of cells of the frame (width × height).
    pub fn cell_count(&self) -> usize {
        self.width as usize * self.height as usize
    }

    /// The rows of the frame, each of them holding `width` cells.
    pub fn rows(&self) -> &[Vec<Cell>] {
        &self.rows
    }

    /// The cells of the `y`-th row.
    pub fn row(&self, y: usize) -> Option<&[Cell]> {
        self.rows.get(y).map(Vec::as_slice)
    }

    /// Whether a patch between `self` and `other` may be computed
    /// and applied: both frames must have the same format version,
    /// the same dimensions and the same skin fingerprint.
    ///
    /// When this returns `false`, a full frame must be sent instead
    /// of trying to apply a diff.
    pub fn compatible_with(&self, other: &Frame) -> bool {
        self.version == other.version
            && self.width == other.width
            && self.height == other.height
            && self.skin_fingerprint == other.skin_fingerprint
    }

    /// Compute the minimal row-level patch transforming `prev`
    /// into `self`. If `prev` is `None` or isn't compatible with
    /// `self`, the returned patch rewrites everything.
    pub fn patch_since(&self, prev: Option<&Frame>) -> FramePatch {
        FramePatch::between(prev, self)
    }

    /// Write the full frame (equivalent to building the patch from
    /// nothing and applying it).
    pub fn write_on<W: std::io::Write>(&self, w: &mut W, area: &crate::Area) -> crate::errors::Result<()> {
        FramePatch::between(None, self).write_on(w, area)
    }

    /// Fill a row from the ANSI string produced by the standard
    /// termimad rendering of that row.
    ///
    /// Graphemes are laid out on the grid according to their
    /// display width. Wide graphemes occupy two cells. Tabs are
    /// expanded to the next multiple of 8 columns. Content which
    /// would overflow the width (including half of a wide grapheme)
    /// is dropped, leaving blank cells, so no stale cell can remain.
    pub(crate) fn set_row_from_ansi(&mut self, y: usize, ansi: &str) {
        let width = self.width as usize;
        let Some(row) = self.rows.get_mut(y) else {
            return;
        };
        let mut x = 0usize;
        let mut last_real_cell = None;
        for (grapheme, style) in ansi::parse_styled_graphemes(ansi) {
            let style = CellStyle::from(style);
            if grapheme == "\t" {
                let spaces = 8 - (x % 8);
                for _ in 0..spaces {
                    if x < width {
                        row[x] = Cell {
                            symbol: " ".to_string(),
                            style,
                        };
                        last_real_cell = Some(x);
                        x += 1;
                    }
                }
                continue;
            }
            let gw = grapheme_width(&grapheme);
            if gw == 0 {
                // zero-width grapheme (combining mark, unjoined ZWJ
                // sequence part, ...): merge it into the previous cell
                if let Some(idx) = last_real_cell {
                    row[idx].symbol.push_str(&grapheme);
                }
                continue;
            }
            if x + gw > width {
                // doesn't fit (eg half a wide grapheme at the end of
                // the row): drop it, the cells stay blank
                continue;
            }
            row[x] = Cell {
                symbol: grapheme,
                style,
            };
            last_real_cell = Some(x);
            x += 1;
            for _ in 1..gw {
                row[x] = Cell::continuation(style);
                x += 1;
            }
        }
    }
}

/// The display width of a grapheme cluster, in terminal columns.
///
/// Zero-width joiner sequences (eg family emojis) are rendered as
/// one double-width glyph by modern terminals.
pub(crate) fn grapheme_width(grapheme: &str) -> usize {
    if grapheme.contains('\u{200D}') {
        2
    } else {
        UnicodeWidthStr::width(grapheme)
    }
}

/// Compute a fingerprint of a skin, suitable to detect any change
/// which might affect the rendering. The fingerprint is stable for
/// a given skin content, whatever the instance or the process.
pub fn skin_fingerprint(skin: &MadSkin) -> u64 {
    let mut hasher = DefaultHasher::new();
    macro_rules! hash_field {
        ($field:expr) => {
            hasher.write(format!("{:?}", $field).as_bytes());
        };
    }
    hash_field!(skin.paragraph);
    hash_field!(skin.bold);
    hash_field!(skin.italic);
    hash_field!(skin.strikeout);
    hash_field!(skin.inline_code);
    hash_field!(skin.code_block);
    hash_field!(skin.headers);
    hash_field!(skin.scrollbar);
    hash_field!(skin.table);
    hash_field!(skin.bullet);
    hash_field!(skin.ordered_item_styles);
    hash_field!(skin.quote_mark);
    hash_field!(skin.horizontal_rule);
    hash_field!(skin.ellipsis);
    hash_field!(skin.table_border_chars);
    hash_field!(skin.list_items_indentation_mode);
    #[cfg(feature = "special-renders")]
    {
        // hash maps don't iterate in a deterministic order, so the
        // entries are sorted before being hashed
        let mut entries: Vec<String> = skin
            .special_chars
            .iter()
            .map(|(k, v)| format!("{:?}={:?}", k, v))
            .collect();
        entries.sort();
        for entry in entries {
            hasher.write(entry.as_bytes());
        }
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::MadSkin,
    };

    #[test]
    fn wide_and_zero_width_layout() {
        let mut frame = Frame::new(8, 1, 0);
        frame.set_row_from_ansi(0, "a好b");
        let row = &frame.rows()[0];
        assert_eq!(row[0].symbol, "a");
        assert_eq!(row[1].symbol, "好");
        assert!(row[2].is_continuation());
        assert_eq!(row[3].symbol, "b");
        assert_eq!(row[4].symbol, " ");
    }

    #[test]
    fn combining_and_zwj() {
        let mut frame = Frame::new(10, 1, 0);
        // "é" as e + combining acute, then a ZWJ emoji sequence
        frame.set_row_from_ansi(0, "e\u{301}👨\u{200D}👩\u{200D}👧x");
        let row = &frame.rows()[0];
        assert_eq!(row[0].symbol, "e\u{301}");
        assert_eq!(row[1].symbol, "👨\u{200D}👩\u{200D}👧");
        assert!(row[2].is_continuation());
        assert_eq!(row[3].symbol, "x");
    }

    #[test]
    fn cropped_half_grapheme_leaves_blank() {
        let mut frame = Frame::new(3, 1, 0);
        // "好" would start at the last column: it must be dropped
        frame.set_row_from_ansi(0, "ab好");
        let row = &frame.rows()[0];
        assert_eq!(row[0].symbol, "a");
        assert_eq!(row[1].symbol, "b");
        assert_eq!(row[2].symbol, " ");
    }

    #[test]
    fn tab_expansion() {
        let mut frame = Frame::new(12, 1, 0);
        frame.set_row_from_ansi(0, "ab\tc");
        let row = &frame.rows()[0];
        assert_eq!(row[0].symbol, "a");
        assert_eq!(row[1].symbol, "b");
        for cell in &row[2..8] {
            assert_eq!(cell.symbol, " ");
        }
        assert_eq!(row[8].symbol, "c");
    }

    #[test]
    fn skin_fingerprint_stability() {
        let skin1 = MadSkin::default();
        let skin2 = MadSkin::default();
        assert_eq!(skin_fingerprint(&skin1), skin_fingerprint(&skin2));
        let mut skin3 = MadSkin::default();
        skin3.bold.set_fg(crate::crossterm::style::Color::Red);
        assert_ne!(skin_fingerprint(&skin1), skin_fingerprint(&skin3));
    }
}
