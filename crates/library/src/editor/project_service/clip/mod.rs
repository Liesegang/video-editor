//! Clip factories, timing, placement, and Track-membership commands.

use super::lifecycle::ProjectManager;
use super::node::{DEFAULT_SHAPE_PATH, DEFAULT_SKSL_SHADER, DEFAULT_TEXT_FONT, MediaNodeRequest};
use crate::editor::handlers;
use crate::editor::handlers::clip_handler::ClipBundle;
use crate::error::LibraryError;
use crate::model::Clip;
use crate::model::property::PropertyValue;
use ordered_float::OrderedFloat;
use uuid::Uuid;

mod raster_graph;

impl ProjectManager {
    pub fn create_audio_clip(
        &self,
        asset_id: Uuid,
        file_path: &str,
        start_time: f64,
        duration: f64,
        source_start_time: f64,
        speed: f64,
    ) -> Result<ClipBundle, LibraryError> {
        let mut clip = Clip::new("Audio Clip", start_time, duration);
        clip.update_timing_property(
            crate::model::node::CLIP_START_TIME_PROPERTY,
            PropertyValue::Number(OrderedFloat(start_time)),
        )
        .map_err(LibraryError::Project)?;
        clip.update_timing_property(
            crate::model::node::CLIP_DURATION_PROPERTY,
            PropertyValue::Number(OrderedFloat(duration)),
        )
        .map_err(LibraryError::Project)?;
        clip.update_timing_property(
            crate::model::node::CLIP_TRIM_IN_PROPERTY,
            PropertyValue::Number(OrderedFloat(source_start_time)),
        )
        .map_err(LibraryError::Project)?;
        clip.update_timing_property(
            crate::model::node::CLIP_TIME_STRETCH_PROPERTY,
            PropertyValue::Number(OrderedFloat(speed)),
        )
        .map_err(LibraryError::Project)?;
        let node = self.create_media_node(
            "Audio",
            MediaNodeRequest::Audio {
                asset_id,
                audio_stream_index: None,
                file_path: file_path.to_string(),
            },
            0,
            0,
            0,
            0,
        )?;

        Ok(ClipBundle::with_audio_node(clip, node))
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "detached video Clip creation requires source timing and canvas dimensions atomically"
    )]
    pub fn create_video_clip(
        &self,
        asset_id: Uuid,
        file_path: &str,
        start_time: f64,
        duration: f64,
        source_start_time: f64,
        speed: f64,
        canvas_width: u32,
        canvas_height: u32,
    ) -> Result<ClipBundle, LibraryError> {
        // Calculate media dimensions (placeholder or fetch from the Asset when available).
        // Ideally we fetch asset metadata, but avoiding async or lock here if possible. ProjectService usually has asset info.
        let media_width = canvas_width as u64; // Fallback
        let media_height = canvas_height as u64;

        let mut clip = Clip::new("Video Clip", start_time, duration);
        clip.update_timing_property(
            crate::model::node::CLIP_START_TIME_PROPERTY,
            PropertyValue::Number(OrderedFloat(start_time)),
        )
        .map_err(LibraryError::Project)?;
        clip.update_timing_property(
            crate::model::node::CLIP_DURATION_PROPERTY,
            PropertyValue::Number(OrderedFloat(duration)),
        )
        .map_err(LibraryError::Project)?;
        clip.update_timing_property(
            crate::model::node::CLIP_TRIM_IN_PROPERTY,
            PropertyValue::Number(OrderedFloat(source_start_time)),
        )
        .map_err(LibraryError::Project)?;
        clip.update_timing_property(
            crate::model::node::CLIP_TIME_STRETCH_PROPERTY,
            PropertyValue::Number(OrderedFloat(speed)),
        )
        .map_err(LibraryError::Project)?;
        let node = self.create_media_node(
            "Video",
            MediaNodeRequest::Video {
                asset_id,
                file_path: file_path.to_string(),
                stream_index: None,
                audio_stream_index: None,
                outputs: crate::model::MediaOutputSelection::ImageAndAudio,
            },
            u64::from(canvas_width),
            u64::from(canvas_height),
            media_width,
            media_height,
        )?;

        self.wrap_positioned_av_clip(
            clip,
            node,
            [u64::from(canvas_width), u64::from(canvas_height)],
            [media_width, media_height],
        )
    }

    pub fn create_image_clip(
        &self,
        asset_id: Uuid,
        file_path: &str,
        start_time: f64,
        duration: f64,
        canvas_width: u32,
        canvas_height: u32,
    ) -> Result<ClipBundle, LibraryError> {
        let node = self.create_media_node(
            "Image",
            MediaNodeRequest::Image {
                asset_id,
                file_path: file_path.to_string(),
            },
            u64::from(canvas_width),
            u64::from(canvas_height),
            u64::from(canvas_width),
            u64::from(canvas_height),
        )?;

        self.wrap_positioned_image_clip(
            Clip::new("Image Clip", start_time, duration),
            node,
            [u64::from(canvas_width), u64::from(canvas_height)],
            [u64::from(canvas_width), u64::from(canvas_height)],
        )
    }

    pub fn create_text_clip(
        &self,
        text: &str,
        start_time: f64,
        duration: f64,
        canvas_width: u32,
        canvas_height: u32,
    ) -> Result<ClipBundle, LibraryError> {
        let graph = self.create_text_graph(
            text,
            DEFAULT_TEXT_FONT,
            u64::from(canvas_width),
            u64::from(canvas_height),
        )?;

        Ok(ClipBundle {
            clip: Clip::new("Text Clip", start_time, duration),
            graph,
        })
    }

    pub fn create_shape_clip(
        &self,
        start_time: f64,
        duration: f64,
        canvas_width: u32,
        canvas_height: u32,
    ) -> Result<ClipBundle, LibraryError> {
        let graph = self.create_shape_graph(
            DEFAULT_SHAPE_PATH,
            u64::from(canvas_width),
            u64::from(canvas_height),
            100,
            100,
        )?;

        Ok(ClipBundle {
            clip: Clip::new("Shape Clip", start_time, duration),
            graph,
        })
    }

    pub fn create_sksl_clip(
        &self,
        start_time: f64,
        duration: f64,
        canvas_width: u32,
        canvas_height: u32,
    ) -> Result<ClipBundle, LibraryError> {
        let node = self.create_sksl_node(
            DEFAULT_SKSL_SHADER,
            u64::from(canvas_width),
            u64::from(canvas_height),
        )?;

        self.wrap_positioned_image_clip(
            Clip::new("SkSL Clip", start_time, duration),
            node,
            [u64::from(canvas_width), u64::from(canvas_height)],
            [u64::from(canvas_width), u64::from(canvas_height)],
        )
    }

    pub fn add_clip_to_track(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        bundle: ClipBundle,
        insert_index: Option<usize>,
    ) -> Result<Uuid, LibraryError> {
        handlers::clip_handler::ClipHandler::add_clip_to_track(
            &self.project,
            composition_id,
            track_id,
            bundle,
            insert_index,
        )
    }

    pub fn remove_clip_from_track(
        &self,
        track_id: Uuid,
        clip_id: Uuid,
    ) -> Result<(), LibraryError> {
        handlers::clip_handler::ClipHandler::remove_clip_from_track(
            &self.project,
            track_id,
            clip_id,
        )
    }

    pub fn update_clip_timing(
        &self,
        clip_id: Uuid,
        start_time: f64,
        duration: f64,
        trim_in: f64,
    ) -> Result<(), LibraryError> {
        handlers::clip_handler::ClipHandler::update_clip_timing(
            &self.project,
            clip_id,
            start_time,
            duration,
            trim_in,
        )
    }

    pub fn move_clip_to_track(
        &self,
        composition_id: Uuid,
        source_track_id: Uuid,
        clip_id: Uuid,
        target_track_id: Uuid,
        new_start_time: f64,
    ) -> Result<(), LibraryError> {
        handlers::clip_handler::ClipHandler::move_clip_to_track(
            &self.project,
            composition_id,
            source_track_id,
            clip_id,
            target_track_id,
            new_start_time,
        )
    }

    pub fn move_clip_to_track_at_index(
        &self,
        composition_id: Uuid,
        source_track_id: Uuid,
        clip_id: Uuid,
        target_track_id: Uuid,
        new_start_time: f64,
        target_index: Option<usize>,
    ) -> Result<(), LibraryError> {
        handlers::clip_handler::ClipHandler::move_clip_to_track_at_index(
            &self.project,
            composition_id,
            source_track_id,
            clip_id,
            target_track_id,
            new_start_time,
            target_index,
        )
    }
}
