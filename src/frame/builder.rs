use {
    super::{
        Cell,
        CellStyle,
        Frame,
    },
    crate::{
        area::Area,
        skin::MadSkin,
    },
    std::io,
    unicode_segmentation::UnicodeSegmentation,
    unicode_width::UnicodeWidthStr,
};

/// A writer interpreting ANSI escape sequences and printable
/// text to build a [`Frame`].
///
/// It understands the sequences termimad and crossterm emit
/// when rendering (SGR styles, cursor moves, erasures) and
/// emulates a fixed size terminal screen, with correct
/// handling of wide graphemes, combining characters, zero
/// width joiners and tabulations.
///
/// Anything written through the `std::io::Write`
/// implementation is buffered then interpreted on
/// [`finish`](Self::finish), so partial escape sequences
/// split across several writes are correctly handled.
pub struct FrameBuilder {
    frame: Frame,
    /// absolute cursor column
    col: usize,
    /// absolute cursor row
    row: usize,
    style: CellStyle,
    /// set when a grapheme had to be cropped at the right
    /// edge: following printable characters of the line are
    /// discarded, mirroring `CropWriter`
    line_blocked: bool,
    buf: Vec<u8>,
}

impl FrameBuilder {
    /// Build a builder for a blank frame covering the given area
    pub fn new(area: &Area, skin: &MadSkin) -> Self {
        Self::seeded(Frame::blank(area, skin))
    }
    /// Build a builder starting from an existing frame.
    ///
    /// This is mostly useful to apply a patch on a previously
    /// captured frame, eg. to check or simulate a restoration.
    pub fn seeded(frame: Frame) -> Self {
        Self {
            frame,
            col: 0,
            row: 0,
            style: CellStyle::default(),
            line_blocked: false,
            buf: Vec::new(),
        }
    }
    /// Interpret everything which was written and return
    /// the resulting frame.
    pub fn finish(mut self) -> Frame {
        self.interpret_buffer();
        self.frame
    }
    fn left(&self) -> usize {
        self.frame.left as usize
    }
    fn top(&self) -> usize {
        self.frame.top as usize
    }
    fn right(&self) -> usize {
        self.left() + self.frame.width as usize
    }
    fn bottom(&self) -> usize {
        self.top() + self.frame.height as usize
    }
    /// Convert absolute screen coordinates into frame
    /// relative ones, if they fall into the frame
    fn rel(&self, row: usize, col: usize) -> Option<(usize, usize)> {
        if row >= self.top() && row < self.bottom() && col >= self.left() && col < self.right() {
            Some((row - self.top(), col - self.left()))
        } else {
            None
        }
    }
    fn interpret_buffer(&mut self) {
        let input = String::from_utf8_lossy(&self.buf).into_owned();
        self.buf.clear();
        let mut pos = 0;
        while pos < input.len() {
            let rest = &input[pos..];
            let ch = rest.chars().next().unwrap();
            match ch {
                '\x1b' => {
                    pos += self.interpret_escape(rest);
                }
                '\n' => {
                    self.row += 1;
                    self.col = 0;
                    self.line_blocked = false;
                    pos += 1;
                }
                '\r' => {
                    self.col = 0;
                    pos += 1;
                }
                '\t' => {
                    self.put_tab();
                    pos += 1;
                }
                c if (c as u32) < 0x20 || c == '\x7f' => {
                    // other control characters are ignored
                    pos += c.len_utf8();
                }
                _ => {
                    let grapheme = rest.graphemes(true).next().unwrap();
                    self.put_grapheme(grapheme);
                    pos += grapheme.len();
                }
            }
        }
    }
    /// Interpret an escape sequence at the start of `rest`
    /// (which starts with ESC). Return the consumed length.
    fn interpret_escape(&mut self, rest: &str) -> usize {
        let bytes = rest.as_bytes();
        if bytes.len() < 2 {
            return 1;
        }
        match bytes[1] {
            b'[' => self.interpret_csi(rest),
            b']' => {
                // OSC: consume until BEL or ST (ESC \)
                let mut end = 2;
                while end < bytes.len() {
                    if bytes[end] == b'\x07' {
                        return end + 1;
                    }
                    if bytes[end] == b'\x1b' && end + 1 < bytes.len() && bytes[end + 1] == b'\\' {
                        return end + 2;
                    }
                    end += 1;
                }
                end
            }
            b'(' | b')' | b'*' | b'+' => {
                // charset selection: ESC + 2 bytes
                3.min(bytes.len())
            }
            _ => 2,
        }
    }
    /// Interpret a CSI sequence. Return the consumed length.
    fn interpret_csi(&mut self, rest: &str) -> usize {
        let bytes = rest.as_bytes();
        let mut pos = 2;
        let params_start = pos;
        while pos < bytes.len() && (0x30..=0x3f).contains(&bytes[pos]) {
            pos += 1;
        }
        let params_end = pos;
        while pos < bytes.len() && (0x20..=0x2f).contains(&bytes[pos]) {
            pos += 1;
        }
        if pos >= bytes.len() {
            // unterminated sequence at end of input
            return pos;
        }
        let final_byte = bytes[pos];
        if !(0x40..=0x7e).contains(&final_byte) {
            return pos + 1;
        }
        let params_str = &rest[params_start..params_end];
        // DEC private sequences (eg "?25l") are ignored
        if params_str.starts_with('?') || params_str.starts_with('>') {
            return pos + 1;
        }
        let params: Vec<u16> = if params_str.is_empty() {
            Vec::new()
        } else {
            params_str
                .split(';')
                .map(|part| {
                    // sub-parameters (eg "4:2") are reduced to
                    // their main parameter
                    part.split(':').next().unwrap_or("").parse().unwrap_or(0)
                })
                .collect()
        };
        let param = |idx: usize, default: u16| -> u16 {
            match params.get(idx) {
                Some(&v) if v > 0 => v,
                _ => default,
            }
        };
        match final_byte {
            b'm' => self.style.apply_sgr(&params),
            b'H' | b'f' => {
                self.row = param(0, 1).saturating_sub(1) as usize;
                self.col = param(1, 1).saturating_sub(1) as usize;
                self.line_blocked = false;
            }
            b'A' => {
                self.row = self.row.saturating_sub(param(0, 1) as usize);
                self.line_blocked = false;
            }
            b'B' | b'e' => {
                self.row += param(0, 1) as usize;
                self.line_blocked = false;
            }
            b'C' | b'a' => {
                self.col += param(0, 1) as usize;
                self.line_blocked = false;
            }
            b'D' => {
                self.col = self.col.saturating_sub(param(0, 1) as usize);
                self.line_blocked = false;
            }
            b'E' => {
                self.row += param(0, 1) as usize;
                self.col = 0;
                self.line_blocked = false;
            }
            b'F' => {
                self.row = self.row.saturating_sub(param(0, 1) as usize);
                self.col = 0;
                self.line_blocked = false;
            }
            b'G' | b'`' => {
                self.col = param(0, 1).saturating_sub(1) as usize;
                self.line_blocked = false;
            }
            b'd' => {
                self.row = param(0, 1).saturating_sub(1) as usize;
                self.line_blocked = false;
            }
            b'J' => {
                let mode = params.first().copied().unwrap_or(0);
                self.erase_display(mode);
            }
            b'K' => {
                let mode = params.first().copied().unwrap_or(0);
                self.erase_line(mode);
            }
            _ => {}
        }
        pos + 1
    }
    fn erase_display(&mut self, mode: u16) {
        match mode {
            0 => {
                self.erase_line(0);
                for row in (self.row + 1)..self.bottom() {
                    self.erase_row(row);
                }
            }
            1 => {
                for row in self.top()..self.row.min(self.bottom()) {
                    self.erase_row(row);
                }
                self.erase_line(1);
            }
            _ => {
                for row in self.top()..self.bottom() {
                    self.erase_row(row);
                }
            }
        }
    }
    fn erase_row(&mut self, row: usize) {
        if let Some((rel_row, _)) = self.rel(row, self.left()) {
            let blank = Cell::blank(self.style);
            for cell in &mut self.frame.cells[rel_row] {
                *cell = blank.clone();
            }
        }
    }
    fn erase_line(&mut self, mode: u16) {
        if self.row < self.top() || self.row >= self.bottom() {
            return;
        }
        let rel_row = self.row - self.top();
        let blank = Cell::blank(self.style);
        let width = self.frame.width as usize;
        let start = self.col.saturating_sub(self.left()).min(width);
        let end = (self.col + 1).saturating_sub(self.left()).min(width);
        let row = &mut self.frame.cells[rel_row];
        match mode {
            0 => {
                for cell in &mut row[start..] {
                    *cell = blank.clone();
                }
            }
            1 => {
                for cell in &mut row[..end] {
                    *cell = blank.clone();
                }
            }
            _ => {
                for cell in row {
                    *cell = blank.clone();
                }
            }
        }
    }
    fn put_tab(&mut self) {
        // standard terminal tab stops, every 8 columns
        let next = (self.col / 8 + 1) * 8;
        while self.col < next && self.col < self.right() {
            if let Some((rel_row, rel_col)) = self.rel(self.row, self.col) {
                self.frame.cells[rel_row][rel_col] = Cell::blank(self.style);
            }
            self.col += 1;
        }
    }
    fn put_grapheme(&mut self, grapheme: &str) {
        if self.line_blocked {
            return;
        }
        let width = UnicodeWidthStr::width(grapheme);
        if width == 0 {
            // zero width cluster (combining mark, variation
            // selector, ...): merge it into the cluster of
            // the current or previous cell
            let mut col = self.col;
            if col > 0 {
                col -= 1;
                if let Some((rel_row, rel_col)) = self.rel(self.row, col) {
                    if self.frame.cells[rel_row][rel_col].continuation && rel_col > 0 {
                        self.frame.cells[rel_row][rel_col - 1]
                            .symbol
                            .push_str(grapheme);
                        return;
                    }
                    self.frame.cells[rel_row][rel_col].symbol.push_str(grapheme);
                }
            } else if let Some((rel_row, rel_col)) = self.rel(self.row, col) {
                self.frame.cells[rel_row][rel_col].symbol.push_str(grapheme);
            }
            return;
        }
        if self.col >= self.left() && self.col + width > self.right() {
            // a grapheme (usually a wide one) doesn't fit in
            // the remaining cells of the line: it's cropped.
            // We blank the current cell so that no stale
            // half-grapheme remains, and discard the rest of
            // the line, mirroring CropWriter's behavior.
            if let Some((rel_row, rel_col)) = self.rel(self.row, self.col) {
                self.frame.cells[rel_row][rel_col] = Cell::blank(self.style);
            }
            self.line_blocked = true;
            return;
        }
        if let Some((rel_row, rel_col)) = self.rel(self.row, self.col) {
            self.frame.cells[rel_row][rel_col] = Cell {
                symbol: grapheme.to_string(),
                style: self.style,
                continuation: false,
            };
            if width == 2 {
                self.frame.cells[rel_row][rel_col + 1] = Cell::continuation(self.style);
            }
        }
        self.col += width;
    }
}

impl io::Write for FrameBuilder {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
