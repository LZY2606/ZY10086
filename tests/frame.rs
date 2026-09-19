//! Integration tests of the viewport frame API: capture, diff,
//! patch application, serialization and recovery.

use {
    termimad::{
        crossterm::style::{
            Attribute,
            Color,
        },
        Area,
        Frame,
        FramePatch,
        MadSkin,
        MadView,
        PatchOp,
        TextView,
    },
};

fn enable_colors() {
    // colors may be globally disabled (eg NO_COLOR env)
    termimad::crossterm::style::Colored::set_ansi_color_disabled(false);
}

/// Capture the frame of a markdown text in an area.
fn capture(markdown: &str, area: &Area, skin: &MadSkin, scroll: usize) -> Frame {
    let text = skin.area_text(markdown, area);
    let mut view = TextView::from(area, &text);
    view.set_scroll(scroll);
    view.frame()
}

/// The concatenation of the symbols of a row (continuations skipped).
fn row_string(frame: &Frame, y: usize) -> String {
    let mut s = String::new();
    for cell in &frame.rows()[y] {
        if !cell.is_continuation() {
            s.push_str(&cell.symbol);
        }
    }
    s.trim_end().to_string()
}

/// Apply the patch on a copy of `prev` and check we get `next`,
/// cell by cell.
fn assert_patch_reproduces(prev: &Frame, next: &Frame) -> FramePatch {
    let patch = next.patch_since(Some(prev));
    let mut applied = prev.clone();
    patch.apply_to(&mut applied);
    assert_eq!(
        applied, *next,
        "applying the patch to the previous frame must give the next one"
    );
    patch
}

#[test]
fn single_line_edit_writes_fewer_cells_than_full_frame() {
    enable_colors();
    let skin = MadSkin::default();
    let area = Area::new(0, 0, 30, 6);
    let md1 = "# Title\nline one\nline two\nline three\nline four\nline five";
    let md2 = "# Title\nline one\nline **two** changed\nline three\nline four\nline five";
    let frame1 = capture(md1, &area, &skin, 0);
    let frame2 = capture(md2, &area, &skin, 0);
    let patch = assert_patch_reproduces(&frame1, &frame2);
    assert!(!patch.is_full());
    assert!(!patch.is_empty());
    // a typical single-line edit must write much less than a full frame
    assert!(
        patch.cells_written() < frame1.cell_count(),
        "patch writes {} cells, full frame is {}",
        patch.cells_written(),
        frame1.cell_count()
    );
    assert!(patch.cells_written() <= area.width as usize);
    // the same inputs must produce the same patch (stable order)
    assert_eq!(patch, frame2.patch_since(Some(&frame1)));
}

#[test]
fn rebuilt_lines_give_empty_patch() {
    enable_colors();
    let skin = MadSkin::default();
    let area = Area::new(0, 0, 20, 4);
    let md = "one\ntwo\nthree\nfour";
    // the two frames are built from separately parsed and wrapped
    // texts: the internal line objects are different but the diff
    // is content based, so nothing is rewritten
    let frame1 = capture(md, &area, &skin, 0);
    let frame2 = capture(md, &area, &skin, 0);
    assert!(frame2.patch_since(Some(&frame1)).is_empty());
}

#[test]
fn scroll_only_rewrites_changed_rows() {
    enable_colors();
    let skin = MadSkin::default();
    let area = Area::new(0, 0, 20, 3);
    let md = "start\nsame\nsame\nsame\nend";
    let frame1 = capture(md, &area, &skin, 0);
    let frame2 = capture(md, &area, &skin, 1);
    let patch = assert_patch_reproduces(&frame1, &frame2);
    assert!(!patch.is_full());
    // rows of identical content must not be rewritten just because
    // the view was scrolled and its line objects rebuilt
    let touched_rows: Vec<u16> = patch
        .ops()
        .iter()
        .filter_map(|op| match op {
            PatchOp::MoveTo { y, .. } => Some(*y),
            _ => None,
        })
        .collect();
    assert_eq!(touched_rows, vec![0, 2]);
}

#[test]
fn resize_requires_full_frame() {
    enable_colors();
    let skin = MadSkin::default();
    let md = "some text\nand more";
    let frame1 = capture(md, &Area::new(0, 0, 20, 4), &skin, 0);
    let frame2 = capture(md, &Area::new(0, 0, 30, 4), &skin, 0);
    assert!(!frame2.compatible_with(&frame1));
    let patch = frame2.patch_since(Some(&frame1));
    assert!(patch.is_full());
    // a full patch applied on a blank frame of the new size
    // reproduces the new frame
    let mut blank = Frame::new(30, 4, frame2.skin_fingerprint);
    patch.apply_to(&mut blank);
    assert_eq!(blank, frame2);
}

#[test]
fn skin_change_requires_full_frame() {
    enable_colors();
    let skin1 = MadSkin::default();
    let mut skin2 = MadSkin::default();
    skin2.bold.set_fg(Color::Red);
    let area = Area::new(0, 0, 20, 3);
    let md = "some **bold** text";
    let frame1 = capture(md, &area, &skin1, 0);
    let frame2 = capture(md, &area, &skin2, 0);
    assert_ne!(frame1.skin_fingerprint, frame2.skin_fingerprint);
    assert!(!frame2.compatible_with(&frame1));
    assert!(frame2.patch_since(Some(&frame1)).is_full());
    // same skin, same fingerprint
    let frame3 = capture(md, &area, &skin1, 0);
    assert!(frame3.compatible_with(&frame1));
}

#[test]
fn shortened_text_is_erased() {
    enable_colors();
    let skin = MadSkin::default();
    let area = Area::new(0, 0, 20, 2);
    let frame1 = capture("hello wonderful world", &area, &skin, 0);
    let frame2 = capture("hi", &area, &skin, 0);
    let patch = assert_patch_reproduces(&frame1, &frame2);
    assert!(
        patch
            .ops()
            .iter()
            .any(|op| matches!(op, PatchOp::ClearToEndOfLine)),
        "a shortening must erase the tail, got {:?}",
        patch.ops()
    );
    assert_eq!(row_string(&frame2, 0), "hi");
}

#[test]
fn emoji_and_wide_chars_keep_columns() {
    enable_colors();
    let skin = MadSkin::default();
    let area = Area::new(0, 0, 24, 3);
    let md = "a👍b\ne\u{301}👨\u{200D}👩\u{200D}👧x\n🇫🇷 flag";
    let frame1 = capture(md, &area, &skin, 0);
    let rows = frame1.rows();
    // "👍" is a double-width emoji: b lands on column 3
    assert_eq!(rows[0][0].symbol, "a");
    assert_eq!(rows[0][1].symbol, "👍");
    assert!(rows[0][2].is_continuation());
    assert_eq!(rows[0][3].symbol, "b");
    // combining mark merged into the base char, ZWJ sequence is one
    // double-width grapheme
    assert_eq!(rows[1][0].symbol, "e\u{301}");
    assert_eq!(rows[1][1].symbol, "👨\u{200D}👩\u{200D}👧");
    assert!(rows[1][2].is_continuation());
    assert_eq!(rows[1][3].symbol, "x");
    // the flag is one grapheme, two columns
    assert_eq!(rows[2][0].symbol, "🇫🇷");
    assert!(rows[2][1].is_continuation());
    assert_eq!(rows[2][2].symbol, " ");
    // editing after emojis must not shift columns
    let md2 = "a👍c\ne\u{301}👨\u{200D}👩\u{200D}👧y\n🇫🇷 flag";
    let frame2 = capture(md2, &area, &skin, 0);
    let patch = assert_patch_reproduces(&frame1, &frame2);
    assert!(patch.cells_written() <= 2);
}

#[test]
fn cropped_wide_char_at_row_end_leaves_no_stale_cell() {
    enable_colors();
    let skin = MadSkin::default();
    let area = Area::new(0, 0, 5, 1);
    // "好" would be cut in half at the right border
    let frame = capture("abcd好efgh", &area, &skin, 0);
    let row = &frame.rows()[0];
    assert_eq!(row.len(), 5);
    assert_eq!(row[3].symbol, "d");
    assert!(
        row[4].symbol != "好",
        "half a wide grapheme must not be kept"
    );
}

#[test]
fn table_cropping_stays_in_area() {
    enable_colors();
    let skin = MadSkin::default();
    let area = Area::new(0, 0, 16, 4);
    let md = "|name|description|value|\n|-|-|-|\n|alpha|a quite long description|42|\n|beta|another long one|1337|";
    let frame1 = capture(md, &area, &skin, 0);
    for row in frame1.rows() {
        assert_eq!(row.len(), area.width as usize);
    }
    // a change in the table must give a small, applicable patch
    let md2 = md.replace("42", "43");
    let frame2 = capture(&md2, &area, &skin, 0);
    let patch = assert_patch_reproduces(&frame1, &frame2);
    assert!(patch.cells_written() < frame1.cell_count());
}

#[test]
fn tabs_and_style_switches_keep_columns() {
    enable_colors();
    let skin = MadSkin::default();
    let area = Area::new(0, 0, 30, 3);
    let md = "```\nab\tcd\n```\nnormal **bold** *italic*";
    let frame1 = capture(md, &area, &skin, 0);
    for row in frame1.rows() {
        assert_eq!(row.len(), area.width as usize);
    }
    // the bold word must carry the bold attribute
    let bold_cells = frame1
        .rows()
        .iter()
        .flatten()
        .filter(|cell| cell.style.attributes.has(Attribute::Bold))
        .count();
    assert!(bold_cells >= 4, "the bold word should be styled");
    let md2 = md.replace("bold", "bolder");
    let frame2 = capture(&md2, &area, &skin, 0);
    assert_patch_reproduces(&frame1, &frame2);
}

#[test]
fn serde_roundtrip_and_recovery() {
    enable_colors();
    let skin = MadSkin::default();
    let area = Area::new(0, 0, 25, 4);
    let md = "# Hi\nsome **styled** text\n- one\n- two";
    let frame1 = capture(md, &area, &skin, 0);
    // the frame can be saved and restored
    let json = serde_json::to_string(&frame1).unwrap();
    let restored: Frame = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, frame1);
    assert!(restored.compatible_with(&frame1));
    // after a reconnection, a compatible saved frame is diffed normally
    let frame2 = capture(&md.replace("one", "1"), &area, &skin, 0);
    let patch = frame2.patch_since(Some(&restored));
    assert!(!patch.is_full());
    // but a frame saved with another size must trigger a full frame
    let other_size = capture(md, &Area::new(0, 0, 40, 4), &skin, 0);
    assert!(other_size.patch_since(Some(&restored)).is_full());
    // and so must a frame saved with another skin
    let mut other_skin = MadSkin::default();
    other_skin.italic.set_fg(Color::Green);
    let other_skin_frame = capture(md, &area, &other_skin, 0);
    assert!(other_skin_frame.patch_since(Some(&restored)).is_full());
    // and a frame from another format version
    let mut old_version: Frame = serde_json::from_str(&json).unwrap();
    old_version.version = termimad::FRAME_FORMAT_VERSION + 1;
    assert!(frame2.patch_since(Some(&old_version)).is_full());
}

#[test]
fn mad_view_frame_matches_text_view_frame() {
    enable_colors();
    let skin = MadSkin::default();
    let area = Area::new(0, 0, 20, 4);
    let md = "# title\n* item 1\n* item 2\n* item 3\n* item 4\n* item 5";
    let view = MadView::from(md.to_string(), area.clone(), skin.clone());
    let from_mad_view = view.frame();
    let from_text_view = capture(md, &area, &skin, 0);
    assert_eq!(from_mad_view, from_text_view);
}

#[test]
fn frame_full_write_and_patch_write_are_usable() {
    enable_colors();
    let skin = MadSkin::default();
    let area = Area::new(2, 1, 20, 3);
    let md = "hello **world**";
    let frame1 = capture(md, &area, &skin, 0);
    // the full rendering of a frame writes every row
    let mut out = Vec::new();
    frame1.write_on(&mut out, &area).unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains("hello"));
    assert!(out.contains("\u{1b}["));
    // the patch is written with absolute coordinates
    let frame2 = capture("hello **you**", &area, &skin, 0);
    let patch = frame2.patch_since(Some(&frame1));
    let mut out = Vec::new();
    patch.write_on(&mut out, &area).unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains("you"));
    // the unchanged "hello " prefix is skipped: the first move
    // targets row 2, column 9 (area origin (2,1) + 6 cells)
    assert!(
        out.contains("\u{1b}[2;9H"),
        "patch must be positioned in the area, got {:?}",
        out
    );
}
