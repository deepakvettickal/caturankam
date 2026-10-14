//! Renders `packaging/macos/icon.svg` to the 1024 px PNG the macOS bundle is
//! built from. Run after editing the SVG, and commit both:
//!
//!     cargo run -p caturankam --example render_icon

fn main() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packaging/macos");
    let svg = std::fs::read(format!("{root}/icon.svg")).expect("read icon.svg");
    let tree = resvg::usvg::Tree::from_data(&svg, &Default::default()).expect("parse icon.svg");
    let mut pixmap = resvg::tiny_skia::Pixmap::new(1024, 1024).expect("pixmap");
    resvg::render(&tree, Default::default(), &mut pixmap.as_mut());
    // tiny-skia stores premultiplied alpha; PNG wants straight alpha.
    let rgba: Vec<u8> = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    let out = format!("{root}/icon-1024.png");
    image::save_buffer(&out, &rgba, 1024, 1024, image::ColorType::Rgba8).expect("write png");
    println!("wrote {out}");
}
