//! Renders the app icon SVGs in `packaging/macos` to the PNGs the macOS bundle
//! is built from. Run after editing an SVG, and commit both:
//!
//!     cargo run -p caturankam --example render_icon
//!
//! `icon.svg` is the full drawing; `icon-small.svg` is a plain silhouette for
//! the 16 and 32 px sizes, where the drawing's fine lines turn to grey.

fn main() {
    render("icon.svg", "icon-1024.png", 1024);
    render("icon-small.svg", "icon-small-64.png", 64);
}

fn render(svg: &str, png: &str, size: u32) {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packaging/macos");
    let data = std::fs::read(format!("{root}/{svg}")).expect("read svg");
    let tree = resvg::usvg::Tree::from_data(&data, &Default::default()).expect("parse svg");
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size).expect("pixmap");
    let scale = size as f32 / tree.size().width();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    // tiny-skia stores premultiplied alpha; PNG wants straight alpha.
    let rgba: Vec<u8> = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    let out = format!("{root}/{png}");
    image::save_buffer(&out, &rgba, size, size, image::ColorType::Rgba8).expect("write png");
    println!("wrote {out}");
}
