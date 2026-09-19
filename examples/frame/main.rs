//! Demonstration of the viewport frame API: capture frames, diff
//! them into minimal row-level patches, and recover from a saved
//! frame after a "reconnection".
//!
//! Run it with `cargo run --example frame`.

use {
    termimad::{
        crossterm::style::Color,
        Area,
        Frame,
        MadSkin,
        MadView,
    },
};

fn report(label: &str, frame: &Frame, patch: &termimad::FramePatch) {
    println!(
        "{label}: {} ops, {} cells written (full frame: {} cells, full: {})",
        patch.ops().len(),
        patch.cells_written(),
        frame.cell_count(),
        patch.is_full(),
    );
}

fn main() {
    let skin = MadSkin::default();
    let area = Area::new(0, 0, 40, 8);

    let markdown_v1 = "# Report\n\
        * revenue: **12_000** €\n\
        * growth: 3%\n\
        * status: nominal 👍\n\
        \n\
        |quarter|amount|\n\
        |-|-|\n\
        |Q1|10_000|\n\
        |Q2|12_000|";
    let markdown_v2 = markdown_v1.replace("growth: 3%", "growth: **7%**");

    // first render: no previous frame, everything is written
    let view = MadView::from(markdown_v1.to_string(), area.clone(), skin.clone());
    let frame1 = view.frame();
    let patch = frame1.patch_since(None);
    report("initial render", &frame1, &patch);

    // second render, one line edited: only that line is rewritten
    let view = MadView::from(markdown_v2.clone(), area.clone(), skin.clone());
    let frame2 = view.frame();
    let patch = frame2.patch_since(Some(&frame1));
    report("one-line edit ", &frame2, &patch);

    // the frame can be saved (eg by a remote client) and restored
    let json = serde_json::to_string(&frame1).unwrap();
    println!("serialized frame: {} bytes", json.len());
    let restored: Frame = serde_json::from_str(&json).unwrap();
    let patch = frame2.patch_since(Some(&restored));
    report("after reconnect", &frame2, &patch);

    // a resize or a skin change makes the saved frame incompatible:
    // a full frame is required, the diff is never forced
    let resized = MadView::from(
        markdown_v2.clone(),
        Area::new(0, 0, 60, 8),
        skin.clone(),
    )
    .frame();
    let patch = resized.patch_since(Some(&restored));
    report("after resize   ", &resized, &patch);

    let mut other_skin = skin.clone();
    other_skin.bold.set_fg(Color::Red);
    let reskinned = MadView::from(markdown_v2.clone(), area, other_skin).frame();
    let patch = reskinned.patch_since(Some(&restored));
    report("after skin swap", &reskinned, &patch);

    // applying the patches to the saved frame always reproduces
    // the new frame, cell by cell
    let mut applied = restored.clone();
    frame2.patch_since(Some(&restored)).apply_to(&mut applied);
    assert_eq!(applied, frame2);
    println!("\npatch application checked: frames are cell-identical");

    // and here's what the second version looks like
    println!();
    frame2.write_on(&mut std::io::stdout(), &Area::new(0, 0, 40, 8)).unwrap();
    println!();
}
