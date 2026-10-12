#![allow(clippy::unwrap_used, clippy::expect_used)]

fn main() {
    for family in std::env::args().skip(1) {
        let style = deesainer_doc::TextStyle {
            font_family: family.clone(),
            ..Default::default()
        };
        println!("{family}: {:?}", deesainer_text::font_axes(&style));
    }
}
