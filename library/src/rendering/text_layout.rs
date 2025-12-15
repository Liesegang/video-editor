use skia_safe::textlayout::{FontCollection, ParagraphBuilder, ParagraphStyle, TextStyle};
use skia_safe::{FontMgr, FontStyle};

// create_paragraph_builder removed

pub fn measure_text_width(text: &str, primary_font_name: &str, size: f32) -> f32 {
    let mut font_collection = FontCollection::new();
    font_collection.set_default_font_manager(FontMgr::default(), None);

    let mut text_style = TextStyle::new();
    text_style.set_font_families(&[primary_font_name]);
    text_style.set_font_size(size);

    let mut paragraph_style = ParagraphStyle::new();
    paragraph_style.set_text_style(&text_style);

    let mut builder = ParagraphBuilder::new(&paragraph_style, font_collection);

    builder.add_text(text);

    let mut paragraph = builder.build();
    // Layout with infinite width to get the natural width of the text
    paragraph.layout(f32::MAX);

    // valid width is roughly the rightmost glyph position.
    // max_intrinsic_width() or max_width() usually gives the single-line width.
    paragraph.max_intrinsic_width()
}
