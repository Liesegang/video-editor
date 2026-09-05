use super::*;
use library::model::authoring::{Paint, PaintDefinition};
use library::model::property::ColorSpaceRef;
use std::collections::HashMap;
use std::io;

#[derive(Default)]
struct Snapshot {
    button: Option<Rect>,
    geometry: Option<ColorPickerGeometry>,
    palette_tab_rect: Option<Rect>,
    palette_geometry: Option<PaletteGeometry>,
    values: Vec<ColorValue>,
    palette_intents: Vec<PaletteUiIntent>,
    finished: bool,
    supported: bool,
}

fn pointer_button(position: egui::Pos2, pressed: bool) -> egui::Event {
    pointer_button_kind(position, egui::PointerButton::Primary, pressed)
}

fn pointer_button_kind(
    position: egui::Pos2,
    button: egui::PointerButton,
    pressed: bool,
) -> egui::Event {
    egui::Event::PointerButton {
        pos: position,
        button,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn render(
    context: &egui::Context,
    source: &ColorValue,
    events: Vec<egui::Event>,
    frame: usize,
    snapshot: &mut Snapshot,
) {
    render_with_palette(
        context,
        source,
        &ProjectPalette::default(),
        events,
        frame,
        snapshot,
    );
}

fn render_with_palette(
    context: &egui::Context,
    source: &ColorValue,
    palette: &ProjectPalette,
    events: Vec<egui::Event>,
    frame: usize,
    snapshot: &mut Snapshot,
) {
    render_with_palette_access(context, source, Some(palette), events, frame, snapshot);
}

fn render_with_palette_access(
    context: &egui::Context,
    source: &ColorValue,
    palette: Option<&ProjectPalette>,
    events: Vec<egui::Event>,
    frame: usize,
    snapshot: &mut Snapshot,
) {
    let screen = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
    drop(context.run(
        egui::RawInput {
            screen_rect: Some(screen),
            time: Some(frame as f64 / 60.0),
            events,
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default().show(context, |ui| {
                let edit = color_value_picker(ui, Id::new("color-picker-test"), source, palette);
                snapshot.button = Some(edit.response.rect);
                snapshot.geometry = edit.geometry;
                snapshot.palette_tab_rect = edit.palette_tab_rect;
                snapshot.palette_geometry = edit.palette_geometry;
                snapshot.values.extend(edit.value);
                snapshot.palette_intents.extend(edit.palette_intent);
                snapshot.finished |= edit.finished;
                snapshot.supported = edit.supported;
            });
        },
    ));
}

#[test]
fn palette_tab_is_absent_for_a_legacy_encoded_color_boundary(
) -> Result<(), Box<dyn std::error::Error>> {
    let context = egui::Context::default();
    let source = ColorValue::new(ColorSpaceRef::srgb(), [0.25, 0.5, 0.75, 1.0])?;
    let mut snapshot = Snapshot::default();
    render_with_palette_access(&context, &source, None, Vec::new(), 0, &mut snapshot);
    let button = snapshot
        .button
        .ok_or_else(|| io::Error::other("picker button missing"))?;
    render_with_palette_access(
        &context,
        &source,
        None,
        vec![
            egui::Event::PointerMoved(button.center()),
            pointer_button(button.center(), true),
            pointer_button(button.center(), false),
        ],
        1,
        &mut snapshot,
    );
    render_with_palette_access(&context, &source, None, Vec::new(), 2, &mut snapshot);
    assert!(snapshot.palette_tab_rect.is_none());
    assert!(snapshot.palette_geometry.is_none());
    Ok(())
}

fn click_with_palette(
    context: &egui::Context,
    source: &ColorValue,
    palette: &ProjectPalette,
    position: egui::Pos2,
    frame: &mut usize,
    snapshot: &mut Snapshot,
) {
    click_button_with_palette(
        context,
        source,
        palette,
        position,
        egui::PointerButton::Primary,
        frame,
        snapshot,
    );
}

fn click_button_with_palette(
    context: &egui::Context,
    source: &ColorValue,
    palette: &ProjectPalette,
    position: egui::Pos2,
    button: egui::PointerButton,
    frame: &mut usize,
    snapshot: &mut Snapshot,
) {
    render_with_palette(
        context,
        source,
        palette,
        vec![
            egui::Event::PointerMoved(position),
            pointer_button_kind(position, button, true),
        ],
        *frame,
        snapshot,
    );
    *frame += 1;
    render_with_palette(
        context,
        source,
        palette,
        vec![pointer_button_kind(position, button, false)],
        *frame,
        snapshot,
    );
    *frame += 1;
    render_with_palette(context, source, palette, Vec::new(), *frame, snapshot);
    *frame += 1;
}

fn open_palette_tab(
    context: &egui::Context,
    source: &ColorValue,
    palette: &ProjectPalette,
    snapshot: &mut Snapshot,
    frame: &mut usize,
) -> Result<PaletteGeometry, io::Error> {
    open_picker(context, source, snapshot, frame)?;
    let palette_tab = snapshot
        .palette_tab_rect
        .ok_or_else(|| io::Error::other("Palette tab geometry missing"))?;
    click_with_palette(
        context,
        source,
        palette,
        palette_tab.center(),
        frame,
        snapshot,
    );
    snapshot
        .palette_geometry
        .clone()
        .ok_or_else(|| io::Error::other("Palette tab did not open"))
}

fn open_picker(
    context: &egui::Context,
    source: &ColorValue,
    snapshot: &mut Snapshot,
    frame: &mut usize,
) -> Result<ColorPickerGeometry, io::Error> {
    render(context, source, Vec::new(), *frame, snapshot);
    *frame += 1;
    let button = snapshot
        .button
        .ok_or_else(|| io::Error::other("picker button missing"))?;
    let position = button.center();
    render(
        context,
        source,
        vec![
            egui::Event::PointerMoved(position),
            pointer_button(position, true),
        ],
        *frame,
        snapshot,
    );
    *frame += 1;
    render(
        context,
        source,
        vec![pointer_button(position, false)],
        *frame,
        snapshot,
    );
    *frame += 1;
    render(context, source, Vec::new(), *frame, snapshot);
    *frame += 1;
    snapshot
        .geometry
        .ok_or_else(|| io::Error::other("large picker popup did not open"))
}

#[test]
fn opening_and_closing_hdr_picker_never_rewrites_the_f64_source(
) -> Result<(), Box<dyn std::error::Error>> {
    let context = egui::Context::default();
    context.memory_mut(|memory| memory.set_everything_is_visible(true));
    let source = ColorValue::new(ColorSpaceRef::srgb(), [-0.125, 4.25, 0.333333333333, 0.5])?;
    let original = source.clone();
    let mut snapshot = Snapshot::default();
    let mut frame = 0;
    let geometry = open_picker(&context, &source, &mut snapshot, &mut frame)?;
    assert!(geometry.saturation_value.width() >= 340.0);
    assert!(geometry.saturation_value.height() >= 250.0);
    assert!(snapshot.values.is_empty());

    let outside = egui::pos2(900.0, 740.0);
    render(
        &context,
        &source,
        vec![
            egui::Event::PointerMoved(outside),
            pointer_button(outside, true),
        ],
        frame,
        &mut snapshot,
    );
    frame += 1;
    render(
        &context,
        &source,
        vec![pointer_button(outside, false)],
        frame,
        &mut snapshot,
    );
    assert_eq!(source, original);
    assert!(snapshot.values.is_empty());
    assert!(!snapshot.finished, "a no-op popup must not create history");
    Ok(())
}

#[test]
fn real_palette_gesture_roundtrips_through_shared_linear_srgb_transform(
) -> Result<(), Box<dyn std::error::Error>> {
    let context = egui::Context::default();
    context.memory_mut(|memory| memory.set_everything_is_visible(true));
    let source = ColorValue::new(
        ColorSpaceRef::linear_srgb(),
        [0.21404114048223255, 0.033104766570885055, 0.0, 0.75],
    )?;
    assert_eq!(
        library::color_management::to_display_srgb(&source)?,
        [0.5, 0.2, 0.0, 0.75]
    );
    let mut snapshot = Snapshot::default();
    let mut frame = 0;
    let geometry = open_picker(&context, &source, &mut snapshot, &mut frame)?;
    let position = egui::pos2(
        geometry.saturation_value.left() + geometry.saturation_value.width() * 0.82,
        geometry.saturation_value.top() + geometry.saturation_value.height() * 0.18,
    );
    render(
        &context,
        &source,
        vec![
            egui::Event::PointerMoved(position),
            pointer_button(position, true),
        ],
        frame,
        &mut snapshot,
    );
    frame += 1;
    render(
        &context,
        &source,
        vec![pointer_button(position, false)],
        frame,
        &mut snapshot,
    );
    let edited = snapshot
        .values
        .last()
        .ok_or_else(|| io::Error::other("palette gesture emitted no value"))?;
    assert_eq!(edited.color_space(), &ColorSpaceRef::linear_srgb());
    assert_ne!(edited, &source);
    let display = library::color_management::to_display_srgb(edited)?;
    assert!(display[..3]
        .iter()
        .all(|component| (0.0..=1.0).contains(component)));
    assert!((display[3] - 0.75).abs() < f64::EPSILON);
    Ok(())
}

#[test]
fn authored_space_menu_keeps_the_color_palette_open() -> Result<(), Box<dyn std::error::Error>> {
    let context = egui::Context::default();
    context.memory_mut(|memory| memory.set_everything_is_visible(true));
    let source = ColorValue::new(ColorSpaceRef::srgb(), [0.5, 0.2, 0.0, 0.75])?;
    let mut snapshot = Snapshot::default();
    let mut frame = 0;
    let geometry = open_picker(&context, &source, &mut snapshot, &mut frame)?;
    let position = geometry.authored_space.center();

    render(
        &context,
        &source,
        vec![
            egui::Event::PointerMoved(position),
            pointer_button(position, true),
        ],
        frame,
        &mut snapshot,
    );
    frame += 1;
    render(
        &context,
        &source,
        vec![pointer_button(position, false)],
        frame,
        &mut snapshot,
    );
    frame += 1;
    render(&context, &source, Vec::new(), frame, &mut snapshot);

    assert!(
        snapshot.geometry.is_some(),
        "opening the authored-space submenu must not close its parent palette"
    );
    assert!(!snapshot.finished);
    Ok(())
}

#[test]
fn unsupported_tagged_space_is_explicitly_numeric_only() -> Result<(), Box<dyn std::error::Error>> {
    let context = egui::Context::default();
    let source = ColorValue::new(ColorSpaceRef::new("acescg")?, [0.5, 0.25, 2.0, 1.0])?;
    let mut snapshot = Snapshot::default();
    render(&context, &source, Vec::new(), 0, &mut snapshot);
    assert!(!snapshot.supported);
    assert!(snapshot.geometry.is_none());
    assert!(snapshot.values.is_empty());
    Ok(())
}

#[test]
fn display_edit_in_a_space_switch_frame_targets_the_new_authored_space(
) -> Result<(), Box<dyn std::error::Error>> {
    let encoded = ColorValue::new(ColorSpaceRef::srgb(), [0.5, 0.2, 0.0, 0.75])?;
    let linear =
        library::color_management::transform_color(&encoded, &ColorSpaceRef::linear_srgb())?;
    let display = library::color_management::to_display_srgb(&linear)?;
    let mut draft = PickerDraft::from_source(&linear, display);
    draft.hsva.s = 0.4;
    draft.hsva.v = 0.8;
    let edited = value_from_display_draft(&draft)?;
    assert_eq!(edited.color_space(), &ColorSpaceRef::linear_srgb());
    assert_eq!(
        library::color_management::to_display_srgb(&edited)?[3],
        0.75
    );
    Ok(())
}

fn palette_with(color: ColorValue) -> (ProjectPalette, PaintDefinitionId) {
    let id = PaintDefinitionId::new();
    (
        ProjectPalette {
            definitions: HashMap::from([(
                id,
                PaintDefinition {
                    id,
                    name: "Managed Accent".to_string(),
                    paint: Paint::Solid(color),
                    tags: Vec::new(),
                },
            )]),
            groups: Vec::new(),
            ungrouped_order: vec![id],
        },
        id,
    )
}

#[test]
fn selecting_palette_tab_does_not_edit_the_property() -> Result<(), Box<dyn std::error::Error>> {
    let context = egui::Context::default();
    context.memory_mut(|memory| memory.set_everything_is_visible(true));
    let source = ColorValue::new(ColorSpaceRef::srgb(), [0.2, 0.3, 0.4, 0.5])?;
    let (palette, _) = palette_with(source.clone());
    let mut snapshot = Snapshot::default();
    let mut frame = 0;
    open_palette_tab(&context, &source, &palette, &mut snapshot, &mut frame)?;
    assert!(snapshot.values.is_empty());
    assert!(snapshot.palette_intents.is_empty());
    assert!(!snapshot.finished);
    Ok(())
}

#[test]
fn add_current_returns_an_intent_without_editing_the_property(
) -> Result<(), Box<dyn std::error::Error>> {
    let context = egui::Context::default();
    context.memory_mut(|memory| memory.set_everything_is_visible(true));
    let source = ColorValue::new(ColorSpaceRef::linear_srgb(), [3.0, -0.5, 0.4, 0.75])?;
    let palette = ProjectPalette::default();
    let mut snapshot = Snapshot::default();
    let mut frame = 0;
    let geometry = open_palette_tab(&context, &source, &palette, &mut snapshot, &mut frame)?;
    click_with_palette(
        &context,
        &source,
        &palette,
        geometry.add_current.center(),
        &mut frame,
        &mut snapshot,
    );
    assert!(snapshot.values.is_empty());
    assert_eq!(
        snapshot.palette_intents,
        vec![PaletteUiIntent::AddSolid {
            suggested_name: "Color 1".to_string(),
            color: source,
        }]
    );
    assert!(!snapshot.finished);
    Ok(())
}

#[test]
fn palette_swatch_applies_the_exact_managed_value() -> Result<(), Box<dyn std::error::Error>> {
    let context = egui::Context::default();
    context.memory_mut(|memory| memory.set_everything_is_visible(true));
    let source = ColorValue::new(ColorSpaceRef::srgb(), [0.1, 0.2, 0.3, 1.0])?;
    let exact = ColorValue::new(
        ColorSpaceRef::linear_srgb(),
        [2.75, -0.125, 0.333333333333, 0.625],
    )?;
    let (palette, id) = palette_with(exact.clone());
    let mut snapshot = Snapshot::default();
    let mut frame = 0;
    let geometry = open_palette_tab(&context, &source, &palette, &mut snapshot, &mut frame)?;
    let swatch = geometry
        .swatches
        .iter()
        .find(|(candidate, _)| *candidate == id)
        .map(|(_, rect)| *rect)
        .ok_or_else(|| io::Error::other("Palette swatch missing"))?;
    click_with_palette(
        &context,
        &source,
        &palette,
        swatch.center(),
        &mut frame,
        &mut snapshot,
    );
    assert_eq!(snapshot.values.last(), Some(&exact));
    assert!(snapshot.palette_intents.is_empty());
    assert!(snapshot.finished);
    assert!(
        snapshot
            .palette_geometry
            .as_ref()
            .and_then(|geometry| geometry.context)
            .is_none(),
        "a primary swatch click must not open its context actions"
    );
    Ok(())
}

#[test]
fn secondary_click_opens_floating_actions_and_parent_click_cancels_the_draft(
) -> Result<(), Box<dyn std::error::Error>> {
    let context = egui::Context::default();
    context.memory_mut(|memory| memory.set_everything_is_visible(true));
    let source = ColorValue::new(ColorSpaceRef::srgb(), [0.1, 0.2, 0.3, 1.0])?;
    let (palette, id) = palette_with(source.clone());
    let mut snapshot = Snapshot::default();
    let mut frame = 0;
    let geometry = open_palette_tab(&context, &source, &palette, &mut snapshot, &mut frame)?;
    let swatch = geometry
        .swatches
        .iter()
        .find(|(candidate, _)| *candidate == id)
        .map(|(_, rect)| *rect)
        .ok_or_else(|| io::Error::other("Palette swatch missing"))?;
    click_button_with_palette(
        &context,
        &source,
        &palette,
        swatch.center(),
        egui::PointerButton::Secondary,
        &mut frame,
        &mut snapshot,
    );
    let actions = snapshot
        .palette_geometry
        .as_ref()
        .and_then(|geometry| geometry.context)
        .ok_or_else(|| io::Error::other("floating Palette actions did not open"))?;
    assert!(actions.popup.is_positive());
    assert!(actions.rename_name.is_positive());
    assert!(actions.rename.is_positive());
    assert!(actions.delete.is_positive());
    let draft_id = Id::new("color-picker-test").with(("palette_rename", id));
    assert!(context
        .data(|data| data.get_temp::<String>(draft_id))
        .is_some());

    let palette_tab = snapshot
        .palette_tab_rect
        .ok_or_else(|| io::Error::other("Palette tab geometry missing"))?;
    click_with_palette(
        &context,
        &source,
        &palette,
        palette_tab.center(),
        &mut frame,
        &mut snapshot,
    );
    assert!(snapshot.palette_geometry.is_some(), "parent picker closed");
    assert!(snapshot
        .palette_geometry
        .as_ref()
        .and_then(|geometry| geometry.context)
        .is_none());
    assert!(context
        .data(|data| data.get_temp::<String>(draft_id))
        .is_none());
    assert!(snapshot.values.is_empty());
    assert!(snapshot.palette_intents.is_empty());
    Ok(())
}

#[test]
fn escape_closes_context_and_parent_reopen_does_not_restore_abandoned_actions(
) -> Result<(), Box<dyn std::error::Error>> {
    let context = egui::Context::default();
    context.memory_mut(|memory| memory.set_everything_is_visible(true));
    let source = ColorValue::new(ColorSpaceRef::srgb(), [0.1, 0.2, 0.3, 1.0])?;
    let (palette, id) = palette_with(source.clone());
    let mut snapshot = Snapshot::default();
    let mut frame = 0;
    let geometry = open_palette_tab(&context, &source, &palette, &mut snapshot, &mut frame)?;
    let swatch = geometry.swatches[0].1;
    click_button_with_palette(
        &context,
        &source,
        &palette,
        swatch.center(),
        egui::PointerButton::Secondary,
        &mut frame,
        &mut snapshot,
    );
    let draft_id = Id::new("color-picker-test").with(("palette_rename", id));
    assert!(context
        .data(|data| data.get_temp::<String>(draft_id))
        .is_some());

    render_with_palette(
        &context,
        &source,
        &palette,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        frame,
        &mut snapshot,
    );
    frame += 1;
    render_with_palette(
        &context,
        &source,
        &palette,
        Vec::new(),
        frame,
        &mut snapshot,
    );
    frame += 1;
    assert!(context
        .data(|data| data.get_temp::<String>(draft_id))
        .is_none());

    let button = snapshot
        .button
        .ok_or_else(|| io::Error::other("picker button missing"))?;
    click_with_palette(
        &context,
        &source,
        &palette,
        button.center(),
        &mut frame,
        &mut snapshot,
    );
    let reopened = snapshot
        .palette_geometry
        .as_ref()
        .ok_or_else(|| io::Error::other("Palette did not reopen"))?;
    assert!(reopened.context.is_none());
    Ok(())
}

#[test]
fn context_delete_returns_typed_intent_without_closing_parent_picker(
) -> Result<(), Box<dyn std::error::Error>> {
    let context = egui::Context::default();
    context.memory_mut(|memory| memory.set_everything_is_visible(true));
    let source = ColorValue::new(ColorSpaceRef::srgb(), [0.1, 0.2, 0.3, 1.0])?;
    let (palette, id) = palette_with(source.clone());
    let mut snapshot = Snapshot::default();
    let mut frame = 0;
    let geometry = open_palette_tab(&context, &source, &palette, &mut snapshot, &mut frame)?;
    click_button_with_palette(
        &context,
        &source,
        &palette,
        geometry.swatches[0].1.center(),
        egui::PointerButton::Secondary,
        &mut frame,
        &mut snapshot,
    );
    let delete = snapshot
        .palette_geometry
        .as_ref()
        .and_then(|geometry| geometry.context)
        .map(|actions| actions.delete)
        .ok_or_else(|| io::Error::other("floating Delete action missing"))?;
    click_with_palette(
        &context,
        &source,
        &palette,
        delete.center(),
        &mut frame,
        &mut snapshot,
    );
    assert_eq!(
        snapshot.palette_intents,
        vec![PaletteUiIntent::Delete { id }]
    );
    assert!(snapshot.palette_geometry.is_some(), "parent picker closed");
    assert!(snapshot
        .palette_geometry
        .as_ref()
        .and_then(|geometry| geometry.context)
        .is_none());
    assert!(snapshot.values.is_empty());
    Ok(())
}
