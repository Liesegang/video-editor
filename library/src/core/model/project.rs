use std::collections::HashMap;
use uuid::Uuid;
use ordered_float::OrderedFloat;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core::frame::color::Color;
use crate::core::model::asset::Asset;
use crate::core::model::property::{PropertyMap, Vec2};
use crate::core::model::style::StyleInstance;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Project {
    pub name: String,
    pub compositions: Vec<Composition>,
    #[serde(default)]
    pub assets: Vec<Asset>,
    #[serde(default)]
    pub export: ExportConfig,
}

#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Debug)]
pub struct ExportConfig {
    #[serde(default)]
    pub container: Option<String>,
    #[serde(default)]
    pub codec: Option<String>,
    #[serde(default)]
    pub pixel_format: Option<String>,
    #[serde(default)]
    pub ffmpeg_path: Option<String>,
    #[serde(default)]
    pub parameters: HashMap<String, Value>,
}

impl Project {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            compositions: Vec::new(),
            assets: Vec::new(),
            export: ExportConfig::default(),
        }
    }

    pub fn load(json_str: &str) -> Result<Self, serde_json::Error> {
        let project: Project = serde_json::from_str(json_str)?;
        Ok(project)
    }

    pub fn save(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn add_composition(&mut self, composition: Composition) {
        self.compositions.push(composition);
    }

    pub fn get_composition_mut(&mut self, id: Uuid) -> Option<&mut Composition> {
        self.compositions.iter_mut().find(|c| c.id == id)
    }

    pub fn remove_composition(&mut self, id: Uuid) -> Option<Composition> {
        let index = self.compositions.iter().position(|c| c.id == id)?;
        Some(self.compositions.remove(index))
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Composition {
    pub id: Uuid,
    pub name: String,
    pub width: u64,
    pub height: u64,
    pub fps: f64,
    pub duration: f64,
    pub background_color: Color,
    pub color_profile: String,
    #[serde(default)]
    pub work_area_in: u64,
    #[serde(default)]
    pub work_area_out: u64,

    pub tracks: Vec<Track>,
}

impl Composition {
    pub fn new(name: &str, width: u64, height: u64, fps: f64, duration: f64) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.to_string(),
            width,
            height,
            fps,
            duration,
            background_color: Color {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            },
            color_profile: "sRGB".to_string(),
            work_area_in: 0,
            work_area_out: (duration * fps).ceil() as u64,
            tracks: Vec::new(),
        }
    }

    pub fn add_track(&mut self, track: Track) {
        self.tracks.push(track);
    }

    pub fn get_track_mut(&mut self, id: Uuid) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }

    pub fn remove_track(&mut self, id: Uuid) -> Option<Track> {
        let index = self.tracks.iter().position(|t| t.id == id)?;
        let removed_track = self.tracks.remove(index);
        Some(removed_track)
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Track {
    pub id: Uuid,
    pub name: String,
    pub clips: Vec<TrackClip>,
}

impl Track {
    pub fn new(name: &str) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.to_string(),
            clips: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct TrackClip {
    pub id: Uuid,
    pub reference_id: Option<Uuid>,
    pub kind: TrackClipKind,
    pub in_frame: u64,
    pub out_frame: u64,
    pub source_begin_frame: u64,
    pub duration_frame: Option<u64>,
    pub fps: f64,
    #[serde(default)]
    pub properties: PropertyMap,
    #[serde(default)]
    pub styles: Vec<StyleInstance>,
    #[serde(default)]
    pub effects: Vec<EffectConfig>,
}

impl TrackClip {
    pub fn new(
        id: Uuid,
        reference_id: Option<Uuid>,
        kind: TrackClipKind,
        in_frame: u64,
        out_frame: u64,
        source_begin_frame: u64,
        duration_frame: Option<u64>,
        fps: f64,
        properties: PropertyMap,
        styles: Vec<StyleInstance>,
        effects: Vec<EffectConfig>,
    ) -> Self {
        Self {
            id,
            reference_id,
            kind,
            in_frame,
            out_frame,
            source_begin_frame,
            duration_frame,
            fps,
            properties,
            styles,
            effects,
        }
    }

    // Factory methods
    pub fn create_video(
        file_path: &str,
        in_frame: u64,
        duration_frames: u64, // Source duration
        fps: f64,
        canvas_width: u32,
        canvas_height: u32,
        clip_width: u32,
        clip_height: u32
    ) -> Self {
        let mut props = PropertyMap::new();
        props.set("file_path".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::String(file_path.to_string())));
        // Standard Transforms
        props.set("position".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Vec2(Vec2{x: OrderedFloat(canvas_width as f64 / 2.0), y: OrderedFloat(canvas_height as f64 / 2.0)})));
        props.set("scale".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Vec2(Vec2{x: OrderedFloat(100.0), y: OrderedFloat(100.0)})));
        props.set("rotation".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Number(OrderedFloat(0.0))));
        props.set("anchor".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Vec2(Vec2{x: OrderedFloat(clip_width as f64 / 2.0), y: OrderedFloat(clip_height as f64 / 2.0)})));
        props.set("opacity".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Number(OrderedFloat(100.0))));
        props.set("width".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Number(OrderedFloat(clip_width as f64))));
        props.set("height".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Number(OrderedFloat(clip_height as f64))));

        Self::new(
            Uuid::new_v4(),
            None,
            TrackClipKind::Video,
            in_frame,
            in_frame + duration_frames,
            0,
            Some(duration_frames),
            fps,
            props,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn create_image(
        file_path: &str,
        in_frame: u64,
        out_frame: u64,
        canvas_width: u32,
        canvas_height: u32,
        clip_width: u32,
        clip_height: u32
    ) -> Self {
        let mut props = PropertyMap::new();
        props.set("file_path".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::String(file_path.to_string())));

        props.set("position".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Vec2(Vec2{x: OrderedFloat(canvas_width as f64 / 2.0), y: OrderedFloat(canvas_height as f64 / 2.0)})));
        props.set("scale".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Vec2(Vec2{x: OrderedFloat(100.0), y: OrderedFloat(100.0)})));
        props.set("rotation".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Number(OrderedFloat(0.0))));
        props.set("anchor".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Vec2(Vec2{x: OrderedFloat(clip_width as f64 / 2.0), y: OrderedFloat(clip_height as f64 / 2.0)})));
        props.set("opacity".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Number(OrderedFloat(100.0))));
        props.set("width".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Number(OrderedFloat(clip_width as f64))));
        props.set("height".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Number(OrderedFloat(clip_height as f64))));

        Self::new(
            Uuid::new_v4(),
            None,
            TrackClipKind::Image,
            in_frame,
            out_frame,
            0,
            None,
            0.0,
            props,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn create_text(
        text: &str,
        in_frame: u64,
        out_frame: u64,
        canvas_width: u32,
        canvas_height: u32,
    ) -> Self {
        let mut props = PropertyMap::new();
        props.set("text".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::String(text.to_string())));
        props.set("font_family".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::String("Arial".to_string())));
        props.set("size".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Number(OrderedFloat(100.0))));

        // Styles
        let mut styles = Vec::new();
        let mut fill_props = PropertyMap::new();
        fill_props.set("color".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Color(crate::core::frame::color::Color { r: 255, g: 255, b: 255, a: 255 })));
        styles.push(StyleInstance::new("fill", fill_props));

        props.set("position".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Vec2(Vec2{x: OrderedFloat(canvas_width as f64 / 2.0), y: OrderedFloat(canvas_height as f64 / 2.0)})));
        props.set("scale".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Vec2(Vec2{x: OrderedFloat(100.0), y: OrderedFloat(100.0)})));
        props.set("rotation".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Number(OrderedFloat(0.0))));
        props.set("anchor".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Vec2(Vec2{x: OrderedFloat(0.0), y: OrderedFloat(0.0)})));
        props.set("opacity".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Number(OrderedFloat(100.0))));

        Self::new(
            Uuid::new_v4(),
            None,
            TrackClipKind::Text,
            in_frame,
            out_frame,
            0,
            None,
            0.0,
            props,
            styles,
            Vec::new(),
        )
    }

    pub fn create_shape(
        in_frame: u64,
        out_frame: u64,
        canvas_width: u32,
        canvas_height: u32,
    ) -> Self {
        let mut props = PropertyMap::new();
        let heart_path = "M 50,30 A 20,20 0,0,1 90,30 C 90,55 50,85 50,85 C 50,85 10,55 10,30 A 20,20 0,0,1 50,30 Z";
        props.set("path".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::String(heart_path.to_string())));

        let mut styles = Vec::new();
        let mut fill_props = PropertyMap::new();
        fill_props.set("color".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Color(crate::core::frame::color::Color { r: 255, g: 0, b: 0, a: 255 })));
        styles.push(StyleInstance::new("fill", fill_props));

        props.set("position".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Vec2(Vec2{x: OrderedFloat(canvas_width as f64 / 2.0), y: OrderedFloat(canvas_height as f64 / 2.0)})));
        props.set("scale".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Vec2(Vec2{x: OrderedFloat(100.0), y: OrderedFloat(100.0)})));
        props.set("rotation".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Number(OrderedFloat(0.0))));
        props.set("anchor".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Vec2(Vec2{x: OrderedFloat(50.0), y: OrderedFloat(50.0)})));
        props.set("opacity".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Number(OrderedFloat(100.0))));
        props.set("width".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Number(OrderedFloat(100.0))));
        props.set("height".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Number(OrderedFloat(100.0))));

        Self::new(
            Uuid::new_v4(),
            None,
            TrackClipKind::Shape,
            in_frame,
            out_frame,
            0,
            None,
            0.0,
            props,
            styles,
            Vec::new(),
        )
    }

    pub fn create_sksl(
        in_frame: u64,
        out_frame: u64,
        canvas_width: u32,
        canvas_height: u32,
    ) -> Self {
        let mut props = PropertyMap::new();
        let default_shader = r#"
half4 main(float2 fragCoord) {
    float2 uv = fragCoord / iResolution.xy;
    float3 col = 0.5 + 0.5*cos(iTime+uv.xyx+float3(0,2,4));
    return half4(col,1.0);
}
"#;
        props.set("shader".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::String(default_shader.to_string())));

        props.set("position".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Vec2(Vec2{x: OrderedFloat(canvas_width as f64 / 2.0), y: OrderedFloat(canvas_height as f64 / 2.0)})));
        props.set("scale".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Vec2(Vec2{x: OrderedFloat(100.0), y: OrderedFloat(100.0)})));
        props.set("rotation".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Number(OrderedFloat(0.0))));
        props.set("anchor".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Vec2(Vec2{x: OrderedFloat(canvas_width as f64 / 2.0), y: OrderedFloat(canvas_height as f64 / 2.0)})));
        props.set("opacity".to_string(), crate::core::model::property::Property::constant(crate::core::model::property::PropertyValue::Number(OrderedFloat(100.0))));

        Self::new(
            Uuid::new_v4(),
            None,
            TrackClipKind::SkSL,
            in_frame,
            out_frame,
            0,
            None,
            0.0,
            props,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn default_property_definitions(&self, canvas_width: u32, canvas_height: u32, clip_width: u32, clip_height: u32) -> Vec<crate::extensions::traits::PropertyDefinition> {
        Vec::new()
    }

    pub fn set_constant_property(
        &mut self,
        key: &str,
        value: crate::core::model::property::PropertyValue,
    ) {
        self.properties.set(
            key.to_string(),
            crate::core::model::property::Property::constant(value),
        );
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Copy)]
pub enum TrackClipKind {
    Video,
    Audio,
    Image,
    Text,
    Shape,
    Composition,
    SkSL,
    Adjustment,
}

impl ToString for TrackClipKind {
    fn to_string(&self) -> String {
        match self {
            TrackClipKind::Video => "video".to_string(),
            TrackClipKind::Audio => "audio".to_string(),
            TrackClipKind::Image => "image".to_string(),
            TrackClipKind::Text => "text".to_string(),
            TrackClipKind::Shape => "shape".to_string(),
            TrackClipKind::Composition => "composition".to_string(),
            TrackClipKind::SkSL => "sksl".to_string(),
            TrackClipKind::Adjustment => "adjustment".to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct EffectConfig {
    pub id: Uuid,
    pub effect_type: String,
    pub properties: PropertyMap,
}

impl std::hash::Hash for EffectConfig {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl Eq for EffectConfig {}
