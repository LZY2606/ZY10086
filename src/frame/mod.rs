/*! Viewport frames and minimal patches.

This optional module helps applications running over low
bandwidth remote terminals avoid rewriting the whole screen
when only a few lines changed.

The typical usage is:

* render the current content into a [`Frame`], either with
  [`TextView::render_frame`](crate::TextView::render_frame),
  [`MadView::render_frame`](crate::MadView::render_frame), or
  by writing any termimad rendering into a [`FrameBuilder`]
* keep this frame (it's serializable, so it can be saved and
  reused after a reconnection)
* when the content changes, render the new frame, compute
  [`Frame::diff`], and write the resulting [`FramePatch`]

A frame carries its geometry, a fingerprint of the skin, and
a format version. When a saved frame doesn't match the
current area or skin ([`Frame::check`]), a full frame must be
rendered instead of a patch: [`Frame::diff`] automatically
falls back to a full rewrite in that case.

Everything in this module is pull-based and synchronous:
no background thread, no active loop, and the usual rendering
methods keep writing the full content by default.
*/

mod builder;
mod cell;
mod patch;
mod style;

pub use {
    builder::FrameBuilder,
    cell::Cell,
    patch::{
        FramePatch,
        LinePatch,
        PatchSpan,
    },
    style::{
        CellStyle,
        FrameColor,
    },
};

use {
    crate::{
        area::Area,
        skin::MadSkin,
    },
    serde::{
        Deserialize,
        Serialize,
    },
    std::fmt,
};

/// Version of the frame serialization format.
///
/// Frames saved with a different version must not be
/// patched over: a full frame is required.
pub const FRAME_FORMAT_VERSION: u16 = 1;

/// Why a saved frame can't be used as the base of a patch
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameMismatch {
    /// the frame was saved with another format version
    Version,
    /// the frame doesn't cover the same area
    Geometry,
    /// the frame was rendered with another skin
    SkinFingerprint,
}

impl fmt::Display for FrameMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrameMismatch::Version => write!(f, "frame format version mismatch"),
            FrameMismatch::Geometry => write!(f, "frame geometry mismatch"),
            FrameMismatch::SkinFingerprint => write!(f, "frame skin mismatch"),
        }
    }
}

impl std::error::Error for FrameMismatch {}

/// A snapshot of a viewport: the content and style of every
/// cell of a rectangular screen area, as rendered by
/// termimad.
///
/// Frames are serializable: they can be saved (eg. when a
/// connection drops) and checked later against the current
/// area and skin with [`check`](Self::check) before being
/// used as the base of a patch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    /// format version, see [`FRAME_FORMAT_VERSION`]
    pub version: u16,
    pub left: u16,
    pub top: u16,
    pub width: u16,
    pub height: u16,
    /// fingerprint of the skin used for the rendering
    pub skin_fingerprint: u64,
    pub(crate) cells: Vec<Vec<Cell>>,
}

impl Frame {
    /// Build a blank frame for the given area and skin
    pub fn blank(area: &Area, skin: &MadSkin) -> Self {
        Self {
            version: FRAME_FORMAT_VERSION,
            left: area.left,
            top: area.top,
            width: area.width,
            height: area.height,
            skin_fingerprint: Self::skin_fingerprint(skin),
            cells: vec![vec![Cell::default(); area.width as usize]; area.height as usize],
        }
    }
    /// A fingerprint of the skin, suitable to detect skin
    /// changes between two renderings.
    ///
    /// It's computed from the debug representation of the
    /// skin with a FNV-1a hash, so it's stable across
    /// processes for a given termimad version.
    pub fn skin_fingerprint(skin: &MadSkin) -> u64 {
        let debug = format!("{:?}", skin);
        let mut hash: u64 = 0xcbf29ce484222325;
        for byte in debug.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash
    }
    /// The cell at the given 0-based position, relative to
    /// the frame's top-left corner
    pub fn cell(&self, row: usize, col: usize) -> Option<&Cell> {
        self.cells.get(row).and_then(|row| row.get(col))
    }
    /// The cells of the given 0-based row
    pub fn row_cells(&self, row: usize) -> Option<&[Cell]> {
        self.cells.get(row).map(Vec::as_slice)
    }
    /// Check this frame can be used as the base of a patch
    /// for a rendering in the given area with the given skin.
    ///
    /// On `Err(mismatch)`, a full frame must be written
    /// instead of a patch.
    pub fn check(&self, area: &Area, skin: &MadSkin) -> Result<(), FrameMismatch> {
        if self.version != FRAME_FORMAT_VERSION {
            return Err(FrameMismatch::Version);
        }
        if self.left != area.left
            || self.top != area.top
            || self.width != area.width
            || self.height != area.height
        {
            return Err(FrameMismatch::Geometry);
        }
        if self.skin_fingerprint != Self::skin_fingerprint(skin) {
            return Err(FrameMismatch::SkinFingerprint);
        }
        Ok(())
    }
    /// whether this frame can be diffed with another one
    pub fn is_compatible(&self, other: &Frame) -> bool {
        self.version == other.version
            && self.left == other.left
            && self.top == other.top
            && self.width == other.width
            && self.height == other.height
            && self.skin_fingerprint == other.skin_fingerprint
    }
    /// Compute the minimal line-level patch turning this
    /// frame into `next`.
    ///
    /// If the frames aren't compatible (different version,
    /// geometry or skin), the returned patch is a full
    /// rewrite of `next`.
    ///
    /// The computation is purely cell-based: rebuilding the
    /// internal line objects without changing the rendered
    /// content produces an empty patch, and identical inputs
    /// always produce identical patches.
    pub fn diff(&self, next: &Frame) -> FramePatch {
        if !self.is_compatible(next) {
            return next.full_patch();
        }
        let mut patch = next.patch_header(false);
        for (row, (old_cells, new_cells)) in
            self.cells.iter().zip(next.cells.iter()).enumerate()
        {
            if old_cells != new_cells {
                patch.lines.push(next.line_patch(row));
            }
        }
        patch
    }
    /// A patch writing the whole frame
    pub fn full_patch(&self) -> FramePatch {
        let mut patch = self.patch_header(true);
        for row in 0..self.cells.len() {
            patch.lines.push(self.line_patch(row));
        }
        patch
    }
    fn patch_header(&self, full: bool) -> FramePatch {
        FramePatch::header(
            self.left,
            self.top,
            self.width,
            self.height,
            self.skin_fingerprint,
            full,
        )
    }
    /// Build the patch rewriting the given row: spans of
    /// same-style cells, trailing blank-default cells being
    /// replaced by an end-of-line erasure.
    fn line_patch(&self, row: usize) -> LinePatch {
        let cells = &self.cells[row];
        let mut end = cells.len();
        while end > 0 && cells[end - 1].is_default_blank() {
            end -= 1;
        }
        let erase_to_eol = end < cells.len();
        let mut spans = Vec::new();
        let mut idx = 0;
        while idx < end {
            if cells[idx].continuation {
                idx += 1;
                continue;
            }
            let style = cells[idx].style;
            let mut text = cells[idx].symbol.clone();
            let mut next_idx = idx + 1;
            while next_idx < end {
                if cells[next_idx].continuation {
                    // second half of the previous wide
                    // grapheme, already accounted for
                    next_idx += 1;
                    continue;
                }
                if cells[next_idx].style != style {
                    break;
                }
                text.push_str(&cells[next_idx].symbol);
                next_idx += 1;
            }
            spans.push(PatchSpan { style, text });
            idx = next_idx;
        }
        LinePatch {
            row: row as u16,
            spans,
            erase_to_eol,
        }
    }
}
