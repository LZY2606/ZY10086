use {
    std::io::Write,
    termimad::{
        crossterm::style::Color,
        frame::{
            CellStyle,
            FrameColor,
        },
        Area,
        Frame,
        FrameBuilder,
        FrameMismatch,
        FramePatch,
        MadSkin,
        TextView,
        FRAME_FORMAT_VERSION,
    },
};

fn render(markdown: &str, skin: &MadSkin, area: &Area, scroll: usize) -> Frame {
    let text = skin.area_text(markdown, area);
    let mut view = TextView::from(area, &text);
    view.scroll = scroll;
    view.render_frame()
}

/// Apply a patch on a base frame, simulating a terminal
fn apply(base: &Frame, patch: &FramePatch) -> Frame {
    let mut bytes = Vec::new();
    patch.write_on(&mut bytes).unwrap();
    let mut builder = FrameBuilder::seeded(base.clone());
    builder.write_all(&bytes).unwrap();
    builder.finish()
}

fn sample_markdown() -> &'static str {
    "first line with some content\n\
     second line with **bold** content\n\
     third line with other content\n\
     fourth line, still more content\n\
     fifth line of the sample text\n\
     sixth and last line of text"
}

#[test]
fn single_line_edit_produces_minimal_stable_patch() {
    let skin = MadSkin::default();
    let area = Area::new(2, 1, 44, 6);
    let frame1 = render(sample_markdown(), &skin, &area, 0);
    let edited = sample_markdown().replace("other content", "changed text");
    let frame2 = render(&edited, &skin, &area, 0);

    let patch = frame1.diff(&frame2);
    assert!(!patch.full);
    assert_eq!(patch.lines.len(), 1, "only one row should be rewritten");
    assert_eq!(patch.lines[0].row, 2, "the third row changed");

    // applying the patch gives exactly the full rendering
    assert_eq!(apply(&frame1, &patch), frame2);

    // the patch is deterministic
    let patch_again = frame1.diff(&frame2);
    assert_eq!(patch, patch_again);
    let mut bytes1 = Vec::new();
    let mut bytes2 = Vec::new();
    patch.write_on(&mut bytes1).unwrap();
    patch_again.write_on(&mut bytes2).unwrap();
    assert_eq!(bytes1, bytes2);
    // rows are emitted in stable ascending order
    let mut rows: Vec<u16> = patch.lines.iter().map(|l| l.row).collect();
    let mut sorted = rows.clone();
    sorted.sort();
    assert_eq!(rows, sorted);
    rows.clear();

    // counting: a single line edit writes less cells than the full frame
    let full_cells = frame1.full_patch().cells_written();
    let patch_cells = patch.cells_written();
    assert!(
        patch_cells <= area.width as usize,
        "patch writes {patch_cells} cells, more than one row of {}",
        area.width
    );
    assert!(
        patch_cells < full_cells,
        "patch writes {patch_cells} cells, full frame writes {full_cells}"
    );
}

#[test]
fn rebuilt_lines_produce_empty_patch() {
    let skin = MadSkin::default();
    let area = Area::new(0, 0, 40, 6);
    // the FmtText and its internal line objects are fully
    // rebuilt between the two renderings
    let frame1 = render(sample_markdown(), &skin, &area, 0);
    let frame2 = render(sample_markdown(), &skin, &area, 0);
    assert!(frame1.diff(&frame2).is_empty());
}

#[test]
fn scroll_patch_is_correct() {
    let skin = MadSkin::default();
    let area = Area::new(0, 0, 40, 4);
    let frame1 = render(sample_markdown(), &skin, &area, 0);
    let frame2 = render(sample_markdown(), &skin, &area, 1);
    let patch = frame1.diff(&frame2);
    assert_eq!(apply(&frame1, &patch), frame2);
}

#[test]
fn resize_requires_full_frame() {
    let skin = MadSkin::default();
    let area = Area::new(0, 0, 40, 6);
    let resized = Area::new(0, 0, 30, 6);
    let frame1 = render(sample_markdown(), &skin, &area, 0);
    let frame2 = render(sample_markdown(), &skin, &resized, 0);

    assert_eq!(frame1.check(&resized, &skin), Err(FrameMismatch::Geometry));
    let patch = frame1.diff(&frame2);
    assert!(patch.full, "resized frame must be fully rewritten");
    assert_eq!(patch.lines.len(), resized.height as usize);
    // applying the full patch on a blank frame of the new
    // geometry gives the full rendering
    let blank = Frame::blank(&resized, &skin);
    assert_eq!(apply(&blank, &patch), frame2);
}

#[test]
fn skin_change_requires_full_frame() {
    let skin = MadSkin::default();
    let mut other_skin = MadSkin::default();
    other_skin.bold.set_fg(Color::Red);
    assert_ne!(skin.fingerprint(), other_skin.fingerprint());

    let area = Area::new(0, 0, 40, 6);
    let frame1 = render(sample_markdown(), &skin, &area, 0);
    let frame2 = render(sample_markdown(), &other_skin, &area, 0);

    assert_eq!(
        frame1.check(&area, &other_skin),
        Err(FrameMismatch::SkinFingerprint)
    );
    let patch = frame1.diff(&frame2);
    assert!(patch.full, "skin change must trigger a full rewrite");
    let blank = Frame::blank(&area, &other_skin);
    assert_eq!(apply(&blank, &patch), frame2);
}

#[test]
fn shortened_line_is_erased() {
    let skin = MadSkin::default();
    let area = Area::new(0, 0, 40, 3);
    let frame1 = render("a very long first line\nsecond\nthird", &skin, &area, 0);
    let frame2 = render("short\nsecond\nthird", &skin, &area, 0);

    let patch = frame1.diff(&frame2);
    assert_eq!(patch.lines.len(), 1);
    assert!(
        patch.lines[0].erase_to_eol,
        "shortening a line must erase the end of the row"
    );
    // no stale cell remains after applying the patch
    let applied = apply(&frame1, &patch);
    assert_eq!(applied, frame2);
    assert_eq!(applied.cell(0, 6).unwrap().symbol, " ");
}

#[test]
fn emoji_and_combining_chars_keep_columns() {
    let skin = MadSkin::default();
    let area = Area::new(0, 0, 40, 4);
    let frame1 = render("crab 🦀!\nfamily 👨‍👩‍👧‍👦!\ncafe\u{301} au lait\nplain", &skin, &area, 0);

    // the crab emoji is a wide grapheme: one leading cell
    // and one continuation cell
    let row0 = frame1.row_cells(0).unwrap();
    let crab = row0.iter().position(|c| c.symbol == "🦀").unwrap();
    assert!(row0[crab + 1].continuation);
    assert_eq!(row0[crab + 2].symbol, "!");

    // the ZWJ family emoji is a single cluster of width 2
    let row1 = frame1.row_cells(1).unwrap();
    let family = row1.iter().position(|c| c.symbol == "👨‍👩‍👧‍👦").unwrap();
    assert!(row1[family + 1].continuation);
    assert_eq!(row1[family + 2].symbol, "!");

    // the combining acute accent is merged into the 'e' cell
    let row2 = frame1.row_cells(2).unwrap();
    assert!(row2.iter().any(|c| c.symbol == "e\u{301}"));

    // editing the line after the emojis doesn't break columns
    let frame2 = render(
        "crab 🦀!\nfamily 👨‍👩‍👧‍👦!\ncafe\u{301} au lait, svp\nplain",
        &skin,
        &area,
        0,
    );
    let patch = frame1.diff(&frame2);
    assert_eq!(patch.lines.len(), 1);
    assert_eq!(apply(&frame1, &patch), frame2);
}

#[test]
fn wide_char_replaced_by_narrow_ones_leaves_no_stale_cell() {
    let skin = MadSkin::default();
    let area = Area::new(0, 0, 20, 2);
    let frame1 = render("🦀 rust", &skin, &area, 0);
    let frame2 = render("ab rust", &skin, &area, 0);
    let patch = frame1.diff(&frame2);
    let applied = apply(&frame1, &patch);
    assert_eq!(applied, frame2);
    // the continuation cell of the old emoji is gone
    assert!(!applied.cell(0, 1).unwrap().continuation);
    assert_eq!(applied.cell(0, 1).unwrap().symbol, "b");
}

#[test]
fn cropped_wide_char_leaves_no_stale_cell() {
    let skin = MadSkin::default();
    // area of width 3: the emoji doesn't fit after "ab"
    let area = Area::new(0, 0, 3, 1);
    let frame = render("ab🦀cd", &skin, &area, 0);
    let row = frame.row_cells(0).unwrap();
    assert_eq!(row[0].symbol, "a");
    assert_eq!(row[1].symbol, "b");
    // the cropped emoji must not spill a continuation cell
    // nor let following chars take its place
    assert!(!row[2].continuation);
    assert_eq!(row[2].symbol, " ");
}

#[test]
fn table_cropping_diff() {
    let skin = MadSkin::default();
    let area = Area::new(0, 0, 24, 5);
    let table1 = "|-:|-|:-|\n|aaa|bbb|ccc|\n|dddddd|eeeeee|ffffff|\n|-:|-|:-|";
    let table2 = "|-:|-|:-|\n|aaa|bbb|ccc|\n|dddddd|eeeeee|FFFFF|\n|-:|-|:-|";
    let frame1 = render(table1, &skin, &area, 0);
    let frame2 = render(table2, &skin, &area, 0);
    // the table is cropped to the area width
    for row in 0..area.height as usize {
        assert_eq!(frame1.row_cells(row).unwrap().len(), 24);
    }
    let patch = frame1.diff(&frame2);
    assert!(!patch.full);
    assert_eq!(apply(&frame1, &patch), frame2);
}

#[test]
fn frame_serialization_and_restore() {
    let skin = MadSkin::default();
    let area = Area::new(1, 2, 40, 6);
    let frame1 = render(sample_markdown(), &skin, &area, 0);

    // the frame survives a serialization roundtrip
    let json = serde_json::to_string(&frame1).unwrap();
    let restored: Frame = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, frame1);
    assert_eq!(restored.version, FRAME_FORMAT_VERSION);

    // the restored frame is accepted as a patch base
    assert_eq!(restored.check(&area, &skin), Ok(()));
    let edited = sample_markdown().replace("bold", "strong");
    let frame2 = render(&edited, &skin, &area, 0);
    let patch = restored.diff(&frame2);
    assert!(!patch.full);
    assert_eq!(apply(&restored, &patch), frame2);

    // a frame saved with another geometry is rejected
    let mut tampered: Frame = serde_json::from_str(&json).unwrap();
    tampered.width += 1;
    assert_eq!(tampered.check(&area, &skin), Err(FrameMismatch::Geometry));

    // a frame saved with another format version is rejected
    let mut tampered: Frame = serde_json::from_str(&json).unwrap();
    tampered.version += 1;
    assert_eq!(tampered.check(&area, &skin), Err(FrameMismatch::Version));

    // a frame saved with another skin is rejected
    let mut tampered: Frame = serde_json::from_str(&json).unwrap();
    tampered.skin_fingerprint += 1;
    assert_eq!(
        tampered.check(&area, &skin),
        Err(FrameMismatch::SkinFingerprint)
    );
}

#[test]
fn builder_handles_tabs_styles_and_erasure() {
    let skin = MadSkin::default();
    let area = Area::new(0, 0, 20, 2);
    let mut builder = FrameBuilder::new(&area, &skin);
    // tab stops every 8 columns, SGR styles, erase to EOL
    write!(builder, "a\tb").unwrap();
    write!(builder, "\x1b[1;31mred\x1b[0m plain\x1b[K").unwrap();
    let frame = builder.finish();
    let row = frame.row_cells(0).unwrap();
    assert_eq!(row[0].symbol, "a");
    assert_eq!(row[8].symbol, "b");
    let red = row.iter().position(|c| c.symbol == "r").unwrap();
    assert_eq!(
        row[red].style,
        CellStyle {
            fg: Some(FrameColor::AnsiValue(1)),
            bg: None,
            attrs: 1,
        }
    );
    let plain = row.iter().position(|c| c.symbol == "p").unwrap();
    assert!(row[plain].style.is_default());
}

#[test]
fn builder_crops_wide_char_at_right_edge() {
    let skin = MadSkin::default();
    let area = Area::new(0, 0, 3, 1);
    let mut builder = FrameBuilder::new(&area, &skin);
    write!(builder, "ab🦀d").unwrap();
    let frame = builder.finish();
    let row = frame.row_cells(0).unwrap();
    assert_eq!(row[0].symbol, "a");
    assert_eq!(row[1].symbol, "b");
    assert_eq!(row[2].symbol, " ");
    assert!(!row[2].continuation);
}

#[test]
fn full_patch_rewrite_matches_full_render() {
    let skin = MadSkin::default();
    let area = Area::new(3, 2, 36, 5);
    let frame = render(sample_markdown(), &skin, &area, 0);
    // writing the full patch on a dirty frame must produce
    // exactly the same cells as the full rendering
    let dirty = Frame::blank(&area, &skin);
    let patch = frame.full_patch();
    assert_eq!(apply(&dirty, &patch), frame);
}
