//! Demonstrates the viewport frame API: render a markdown
//! text into a frame, change one line, then compute and
//! apply a minimal patch instead of rewriting the whole
//! screen.
//!
//! Run it with
//!
//!     cargo run --example frame-diff
//!
//! The example is non interactive: it prints the bandwidth
//! statistics and simulates the application of the patch to
//! prove the result is identical to a full rendering.

use {
    std::io::Write,
    termimad::*,
};

fn render(markdown: &str, skin: &MadSkin, area: &Area) -> Frame {
    let text = skin.area_text(markdown, area);
    let view = TextView::from(area, &text);
    view.render_frame()
}

fn main() {
    let skin = MadSkin::default();
    let area = Area::new(0, 0, 50, 8);
    let markdown = "# Status\n\
        * service **api** is *up*\n\
        * service **db** is *up*\n\
        * latency: 12ms\n\
        * users: 418 🦀\n\
        \n\
        |metric|value|\n\
        |-:|-|\n\
        |cpu|12%|";

    // first rendering: a full frame must be written
    let frame1 = render(markdown, &skin, &area);
    let full = frame1.full_patch();
    println!("full rendering writes {} cells", full.cells_written());

    // the frame can be saved (eg. before a disconnection) and
    // checked again at restore time
    let saved = serde_json::to_string(&frame1).unwrap();
    let restored: Frame = serde_json::from_str(&saved).unwrap();
    match restored.check(&area, &skin) {
        Ok(()) => println!("restored frame is reusable ({} bytes of json)", saved.len()),
        Err(mismatch) => println!("restored frame rejected: {mismatch}"),
    }

    // one line changes: only this line is rewritten
    let markdown = markdown.replace("latency: 12ms", "latency: 128ms");
    let frame2 = render(&markdown, &skin, &area);
    let patch = restored.diff(&frame2);
    println!(
        "patch rewrites {} row(s), {} cells (vs {} for a full frame)",
        patch.lines.len(),
        patch.cells_written(),
        frame2.full_patch().cells_written(),
    );

    // simulate a terminal: apply the patch on the previous
    // frame and check we get exactly the full rendering
    let mut bytes = Vec::new();
    patch.write_on(&mut bytes).unwrap();
    println!("patch is {} bytes of ANSI output", bytes.len());
    let mut builder = FrameBuilder::seeded(restored);
    builder.write_all(&bytes).unwrap();
    let applied = builder.finish();
    assert_eq!(applied, frame2);
    println!("applying the patch gives exactly the full rendering");
}
