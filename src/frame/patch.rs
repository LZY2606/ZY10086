//! Minimal row-level patches between two frames.

use {
    super::{
        grapheme_width,
        Cell,
        CellStyle,
        Frame,
    },
    crate::{
        crossterm::{
            cursor::MoveTo,
            style::PrintStyledContent,
            terminal::{
                Clear,
                ClearType,
            },
            QueueableCommand,
        },
        errors::Result,
        Area,
    },
    serde::{
        Deserialize,
        Serialize,
    },
    std::io::Write,
    unicode_segmentation::UnicodeSegmentation,
};

/// One operation of a [`FramePatch`].
///
/// Coordinates are relative to the frame (ie to the area's origin).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PatchOp {
    /// Move the cursor to a cell of the frame.
    MoveTo {
        x: u16,
        y: u16,
    },
    /// Write a run of same-style graphemes. The cursor advances by
    /// the display width of the text (wide graphemes included).
    Print {
        style: CellStyle,
        text: String,
    },
    /// Erase from the cursor to the end of the line. Only emitted
    /// when the remaining cells of the row must be blank with the
    /// default style.
    ClearToEndOfLine,
}

/// A minimal, deterministic, row-level patch between two frames.
///
/// Applying the patch to a terminal which displays the `prev` frame
/// produces exactly the cells of the `next` frame. Operations are
/// emitted in a stable order (rows top to bottom, spans left to
/// right): the same inputs always produce the same patch.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FramePatch {
    /// Whether this patch rewrites the whole frame (because there
    /// was no previous frame, or because size, skin or format
    /// version didn't match).
    full: bool,
    ops: Vec<PatchOp>,
}

impl FramePatch {
    /// Compute the patch transforming `prev` into `next`.
    ///
    /// If `prev` is `None` or isn't compatible with `next` (size,
    /// skin fingerprint or format version differ), a full patch is
    /// returned: diffs are never forced onto an incompatible frame.
    pub fn between(prev: Option<&Frame>, next: &Frame) -> FramePatch {
        let full = prev.map_or(true, |prev| !next.compatible_with(prev));
        let mut ops = Vec::new();
        for y in 0..next.height as usize {
            let prev_row = if full {
                None
            } else {
                prev.and_then(|prev| prev.row(y))
            };
            emit_row_ops(&mut ops, y as u16, prev_row, &next.rows()[y]);
        }
        FramePatch { full, ops }
    }

    /// Whether this patch rewrites the whole frame.
    pub fn is_full(&self) -> bool {
        self.full
    }

    /// Whether this patch does nothing.
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// The operations of the patch, in application order.
    pub fn ops(&self) -> &[PatchOp] {
        &self.ops
    }

    /// Apply the patch to a frame, mutating its cells exactly like
    /// a terminal would: the frame must be the one the patch was
    /// computed from (or a copy of it, eg deserialized after a
    /// reconnection), and it becomes cell-equal to the frame the
    /// patch was computed for.
    pub fn apply_to(&self, frame: &mut Frame) {
        let mut x = 0usize;
        let mut y = 0usize;
        for op in &self.ops {
            match op {
                PatchOp::MoveTo { x: nx, y: ny } => {
                    x = *nx as usize;
                    y = *ny as usize;
                }
                PatchOp::Print { style, text } => {
                    for grapheme in text.graphemes(true) {
                        let gw = grapheme_width(grapheme);
                        if gw == 0 {
                            continue;
                        }
                        if let Some(row) = frame.rows.get_mut(y) {
                            if x < row.len() {
                                row[x] = Cell {
                                    symbol: grapheme.to_string(),
                                    style: *style,
                                };
                                for k in 1..gw {
                                    if x + k < row.len() {
                                        row[x + k] = Cell::continuation(*style);
                                    }
                                }
                            }
                        }
                        x += gw;
                    }
                }
                PatchOp::ClearToEndOfLine => {
                    if let Some(row) = frame.rows.get_mut(y) {
                        for cell in row.iter_mut().skip(x) {
                            *cell = Cell::blank();
                        }
                    }
                }
            }
        }
    }

    /// The number of cells the `Print` operations of this patch
    /// write, in terminal columns (a wide grapheme counts for 2).
    ///
    /// Erasures don't write cells and aren't counted. This is
    /// typically much smaller than [`Frame::cell_count`] for small
    /// edits, which is the point of the diff mode.
    pub fn cells_written(&self) -> usize {
        self.ops
            .iter()
            .map(|op| match op {
                PatchOp::Print { text, .. } => text
                    .graphemes(true)
                    .map(grapheme_width)
                    .sum(),
                _ => 0,
            })
            .sum()
    }

    /// Queue the patch's operations on the given writer, the frame
    /// being displayed in `area`.
    pub fn write_on<W: Write>(&self, w: &mut W, area: &Area) -> Result<()> {
        for op in &self.ops {
            match op {
                PatchOp::MoveTo { x, y } => {
                    w.queue(MoveTo(area.left + x, area.top + y))?;
                }
                PatchOp::Print { style, text } => {
                    let content_style: crate::crossterm::style::ContentStyle =
                        (*style).into();
                    w.queue(PrintStyledContent(content_style.apply(text)))?;
                }
                PatchOp::ClearToEndOfLine => {
                    w.queue(Clear(ClearType::UntilNewLine))?;
                }
            }
        }
        Ok(())
    }

    /// Apply the patch on stdout, the frame being displayed in `area`.
    pub fn write(&self, area: &Area) -> Result<()> {
        let mut stdout = std::io::stdout();
        self.write_on(&mut stdout, area)?;
        stdout.flush()?;
        Ok(())
    }
}

/// Emit the operations updating a row, `prev` being the row
/// currently displayed (`None` for a full rewrite) and `cur` the
/// row to obtain.
fn emit_row_ops(
    ops: &mut Vec<PatchOp>,
    y: u16,
    prev: Option<&[Cell]>,
    cur: &[Cell],
) {
    let width = cur.len();
    let (first, last) = match prev {
        Some(prev) => {
            debug_assert_eq!(prev.len(), width);
            let mut first = 0;
            while first < width && prev[first] == cur[first] {
                first += 1;
            }
            if first == width {
                return; // row unchanged
            }
            let mut last = width - 1;
            while last > first && prev[last] == cur[last] {
                last -= 1;
            }
            (first, last)
        }
        None => (0, width - 1),
    };
    // `first` can't be the continuation cell of a wide grapheme:
    // both cells of a wide grapheme always change together.
    let content_end = cur
        .iter()
        .rposition(|cell| !cell.is_blank() && !cell.is_continuation());
    match content_end {
        Some(end) if end >= first => {
            let write_to = end.min(last);
            emit_spans(ops, y, first as u16, &cur[first..=write_to]);
            if last > write_to {
                // cells which must become blank after the content
                emit_blank_fill(ops, y, (write_to + 1) as u16, cur, write_to + 1, last);
            }
        }
        _ => {
            // the row has no content at or after `first`
            ops.push(PatchOp::MoveTo {
                x: first as u16,
                y,
            });
            emit_blank_fill(ops, y, first as u16, cur, first, last);
        }
    }
}

/// Emit the operations making the cells `from..=to` blank, assuming
/// the cursor is at (`x`, `y`).
///
/// Uses an erasure when all the cells up to the end of the row are
/// blank with the default style, and styled spaces otherwise (an
/// erasure would paint the terminal's default background, not the
/// one of the skin).
fn emit_blank_fill(
    ops: &mut Vec<PatchOp>,
    y: u16,
    x: u16,
    cur: &[Cell],
    from: usize,
    to: usize,
) {
    let erasable = cur[from..]
        .iter()
        .all(|cell| cell.is_blank() && cell.style == CellStyle::default());
    if erasable {
        ops.push(PatchOp::ClearToEndOfLine);
    } else {
        emit_spans(ops, y, x, &cur[from..=to]);
    }
}

/// Emit a `MoveTo` (at `x`, `y`) then one `Print` operation per run
/// of same-style cells. Continuation cells aren't printed: they're
/// covered by the wide grapheme of their start cell.
fn emit_spans(
    ops: &mut Vec<PatchOp>,
    y: u16,
    x: u16,
    cells: &[Cell],
) {
    debug_assert!(!cells.is_empty());
    ops.push(PatchOp::MoveTo { x, y });
    let mut idx = 0;
    while idx < cells.len() {
        let style = cells[idx].style;
        let mut text = String::new();
        while idx < cells.len() && cells[idx].style == style {
            if !cells[idx].is_continuation() {
                text.push_str(&cells[idx].symbol);
            }
            idx += 1;
        }
        if !text.is_empty() {
            ops.push(PatchOp::Print { style, text });
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::frame::Frame,
    };

    fn frame_from_rows(rows: &[&str]) -> Frame {
        frame_from_rows_width(rows, 12)
    }

    fn frame_from_rows_width(rows: &[&str], width: u16) -> Frame {
        let height = rows.len() as u16;
        let mut frame = Frame::new(width, height, 0);
        for (y, row) in rows.iter().enumerate() {
            frame.set_row_from_ansi(y, row);
        }
        frame
    }

    #[test]
    fn no_change_no_op() {
        let a = frame_from_rows(&["hello", "world"]);
        let b = frame_from_rows(&["hello", "world"]);
        let patch = FramePatch::between(Some(&a), &b);
        assert!(patch.is_empty());
        assert!(!patch.is_full());
        assert_eq!(patch.cells_written(), 0);
    }

    #[test]
    fn patch_is_deterministic() {
        let a = frame_from_rows(&["hello", "world"]);
        let b = frame_from_rows(&["hello!", "world"]);
        assert_eq!(
            FramePatch::between(Some(&a), &b),
            FramePatch::between(Some(&a), &b)
        );
    }

    #[test]
    fn incompatible_frames_give_full_patch() {
        let a = frame_from_rows(&["hello"]);
        let mut b = frame_from_rows(&["hello"]);
        b.skin_fingerprint = 42;
        let patch = FramePatch::between(Some(&a), &b);
        assert!(patch.is_full());
        let c = frame_from_rows_width(&["hello"], a.width + 1);
        assert!(FramePatch::between(Some(&a), &c).is_full());
        let mut d = a.clone();
        d.version += 1;
        assert!(FramePatch::between(Some(&a), &d).is_full());
    }

    #[test]
    fn shortened_line_is_erased() {
        let a = frame_from_rows(&["hello world"]);
        let b = frame_from_rows(&["hello"]);
        let patch = FramePatch::between(Some(&a), &b);
        assert!(!patch.is_full());
        assert!(patch
            .ops()
            .iter()
            .any(|op| matches!(op, PatchOp::ClearToEndOfLine)));
        assert!(patch.cells_written() <= 5);
    }

    #[test]
    fn apply_to_reproduces_next_frame() {
        let a = frame_from_rows(&["hello world", "a 好 b", "third line"]);
        let b = frame_from_rows(&["hello", "a 好c", "third line!"]);
        let patch = FramePatch::between(Some(&a), &b);
        let mut applied = a.clone();
        patch.apply_to(&mut applied);
        assert_eq!(applied, b);
        // a full patch applied on a blank frame also works
        let full = FramePatch::between(None, &b);
        assert!(full.is_full());
        let mut blank = Frame::new(b.width, b.height, b.skin_fingerprint);
        full.apply_to(&mut blank);
        assert_eq!(blank, b);
    }
}
