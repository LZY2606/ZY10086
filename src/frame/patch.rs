use {
    super::{
        CellStyle,
        FRAME_FORMAT_VERSION,
    },
    crate::crossterm::{
        cursor::MoveTo,
        style::{
            Attribute,
            Print,
            SetAttribute,
            SetBackgroundColor,
            SetForegroundColor,
        },
        terminal::{
            Clear,
            ClearType,
        },
        QueueableCommand,
    },
    serde::{
        Deserialize,
        Serialize,
    },
    std::io::{
        self,
        Write,
    },
    unicode_width::UnicodeWidthStr,
};

/// A minimal set of changes turning a previously rendered
/// [`Frame`](super::Frame) into a new one.
///
/// A patch is made of whole line rewrites, in stable
/// top-to-bottom order: for every row whose cells changed,
/// a cursor move to the start of the row, styled text spans,
/// and an optional end-of-line erasure (used when the new
/// content is shorter than the old one, so that no stale
/// cell remains).
///
/// Writing a patch with [`write_on`](Self::write_on) produces
/// exactly the same screen, cell by cell, as writing the
/// complete new frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FramePatch {
    pub version: u16,
    pub left: u16,
    pub top: u16,
    pub width: u16,
    pub height: u16,
    pub skin_fingerprint: u64,
    /// whether this patch rewrites every row (either because
    /// it was asked for, or because the previous frame wasn't
    /// compatible with the new one)
    pub full: bool,
    pub lines: Vec<LinePatch>,
}

/// The rewrite of one terminal row
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinePatch {
    /// 0-based row, relative to the frame's top
    pub row: u16,
    pub spans: Vec<PatchSpan>,
    /// whether the rest of the row must be erased after the
    /// spans (the new content is shorter than the old one)
    pub erase_to_eol: bool,
}

/// A run of cells sharing the same style
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatchSpan {
    pub style: CellStyle,
    pub text: String,
}

impl FramePatch {
    pub(crate) fn header(
        left: u16,
        top: u16,
        width: u16,
        height: u16,
        skin_fingerprint: u64,
        full: bool,
    ) -> Self {
        Self {
            version: FRAME_FORMAT_VERSION,
            left,
            top,
            width,
            height,
            skin_fingerprint,
            full,
            lines: Vec::new(),
        }
    }
    /// whether the patch changes nothing
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
    /// the number of terminal cells whose content is written
    /// by this patch (erasures not counted)
    ///
    /// Comparing this with `frame.full_patch().cells_written()`
    /// tells how much bandwidth the diff saves.
    pub fn cells_written(&self) -> usize {
        self.lines
            .iter()
            .flat_map(|line| &line.spans)
            .map(|span| UnicodeWidthStr::width(span.text.as_str()))
            .sum()
    }
    /// Write the patch as ANSI escape sequences and text.
    ///
    /// The terminal is assumed to currently show the frame
    /// this patch was computed from.
    pub fn write_on<W: Write>(&self, w: &mut W) -> io::Result<()> {
        for line in &self.lines {
            w.queue(MoveTo(self.left, self.top + line.row))?;
            w.queue(SetAttribute(Attribute::Reset))?;
            let mut current = CellStyle::default();
            for span in &line.spans {
                if span.style != current {
                    write_style(w, &span.style)?;
                    current = span.style;
                }
                w.queue(Print(&span.text))?;
            }
            if line.erase_to_eol {
                // the erased cells are blank with the default
                // style: make sure the current style is the
                // default one so that the erasure doesn't
                // paint a stale background
                if !current.is_default() {
                    w.queue(SetAttribute(Attribute::Reset))?;
                }
                w.queue(Clear(ClearType::UntilNewLine))?;
            }
        }
        Ok(())
    }
}

/// Emit the SGR sequences switching from any state to the
/// given style
fn write_style<W: Write>(w: &mut W, style: &CellStyle) -> io::Result<()> {
    w.queue(SetAttribute(Attribute::Reset))?;
    if let Some(fg) = style.fg {
        w.queue(SetForegroundColor(fg.into()))?;
    }
    if let Some(bg) = style.bg {
        w.queue(SetBackgroundColor(bg.into()))?;
    }
    for attribute in style.attributes() {
        w.queue(SetAttribute(attribute))?;
    }
    Ok(())
}
