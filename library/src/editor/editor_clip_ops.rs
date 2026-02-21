use super::editor_service::EditorService;
use crate::error::LibraryError;
use crate::model::project::clip::TrackClip;
use crate::model::project::property::PropertyValue;
use crate::model::project::track::TrackData;
use uuid::Uuid;

/// Clip factory, track, clip CRUD, property, and keyframe operations.
impl EditorService {
    // --- Clip Factory Methods ---

    pub fn create_audio_clip(
        &self,
        reference_id: Option<Uuid>,
        file_path: &str,
        in_frame: u64,
        out_frame: u64,
        source_begin_frame: i64,
        duration_frame: u64,
        fps: f64,
    ) -> TrackClip {
        self.project_manager.create_audio_clip(
            reference_id,
            file_path,
            in_frame,
            out_frame,
            source_begin_frame,
            duration_frame,
            fps,
        )
    }

    pub fn create_video_clip(
        &self,
        reference_id: Option<Uuid>,
        file_path: &str,
        in_frame: u64,
        out_frame: u64,
        source_begin_frame: i64,
        duration_frame: u64,
        fps: f64,
        canvas_width: u32,
        canvas_height: u32,
    ) -> Result<TrackClip, LibraryError> {
        self.project_manager.create_video_clip(
            reference_id,
            file_path,
            in_frame,
            out_frame,
            source_begin_frame,
            duration_frame,
            fps,
            canvas_width,
            canvas_height,
        )
    }

    pub fn create_image_clip(
        &self,
        reference_id: Option<Uuid>,
        file_path: &str,
        in_frame: u64,
        out_frame: u64,
        canvas_width: u32,
        canvas_height: u32,
        fps: f64,
    ) -> Result<TrackClip, LibraryError> {
        self.project_manager.create_image_clip(
            reference_id,
            file_path,
            in_frame,
            out_frame,
            canvas_width,
            canvas_height,
            fps,
        )
    }

    pub fn create_text_clip(
        &self,
        text: &str,
        in_frame: u64,
        out_frame: u64,
        canvas_width: u32,
        canvas_height: u32,
        fps: f64,
    ) -> Result<TrackClip, LibraryError> {
        self.project_manager.create_text_clip(
            text,
            in_frame,
            out_frame,
            canvas_width,
            canvas_height,
            fps,
        )
    }

    pub fn create_shape_clip(
        &self,
        in_frame: u64,
        out_frame: u64,
        canvas_width: u32,
        canvas_height: u32,
        fps: f64,
    ) -> Result<TrackClip, LibraryError> {
        self.project_manager.create_shape_clip(
            in_frame,
            out_frame,
            canvas_width,
            canvas_height,
            fps,
        )
    }

    pub fn create_sksl_clip(
        &self,
        in_frame: u64,
        out_frame: u64,
        canvas_width: u32,
        canvas_height: u32,
        fps: f64,
    ) -> Result<TrackClip, LibraryError> {
        self.project_manager
            .create_sksl_clip(in_frame, out_frame, canvas_width, canvas_height, fps)
    }

    // --- Track Operations ---

    pub fn add_track(&self, composition_id: Uuid, track_name: &str) -> Result<Uuid, LibraryError> {
        self.project_manager.add_track(composition_id, track_name)
    }

    pub fn add_track_with_id(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        track_name: &str,
    ) -> Result<Uuid, LibraryError> {
        self.project_manager
            .add_track_with_id(composition_id, track_id, track_name)
    }

    pub fn get_track(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
    ) -> Result<TrackData, LibraryError> {
        self.project_manager.get_track(composition_id, track_id)
    }

    pub fn remove_track(&self, composition_id: Uuid, track_id: Uuid) -> Result<(), LibraryError> {
        self.project_manager.remove_track(composition_id, track_id)
    }

    pub fn add_sub_track(
        &self,
        composition_id: Uuid,
        parent_track_id: Uuid,
        track_name: &str,
    ) -> Result<Uuid, LibraryError> {
        self.project_manager
            .add_sub_track(composition_id, parent_track_id, track_name)
    }

    pub fn rename_track(&self, track_id: Uuid, new_name: &str) -> Result<(), LibraryError> {
        self.project_manager.rename_track(track_id, new_name)
    }

    // --- Clip CRUD ---

    pub fn add_clip_to_track(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip: TrackClip,
        in_frame: u64,
        out_frame: u64,
        insert_index: Option<usize>,
    ) -> Result<Uuid, LibraryError> {
        self.project_manager.add_clip_to_track(
            composition_id,
            track_id,
            clip,
            in_frame,
            out_frame,
            insert_index,
        )
    }

    pub fn remove_clip_from_track(
        &self,
        track_id: Uuid,
        clip_id: Uuid,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .remove_clip_from_track(track_id, clip_id)
    }

    pub fn update_clip_property(
        &self,
        clip_id: Uuid,
        property_key: &str,
        value: PropertyValue,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .update_track_clip_property(clip_id, property_key, value)
    }

    pub fn move_clip_to_track(
        &self,
        composition_id: Uuid,
        source_track_id: Uuid,
        clip_id: Uuid,
        target_track_id: Uuid,
        new_in_frame: u64,
    ) -> Result<(), LibraryError> {
        self.project_manager.move_clip_to_track(
            composition_id,
            source_track_id,
            clip_id,
            target_track_id,
            new_in_frame,
        )
    }

    pub fn move_clip_to_track_at_index(
        &self,
        composition_id: Uuid,
        source_track_id: Uuid,
        clip_id: Uuid,
        target_track_id: Uuid,
        new_in_frame: u64,
        target_index: Option<usize>,
    ) -> Result<(), LibraryError> {
        self.project_manager.move_clip_to_track_at_index(
            composition_id,
            source_track_id,
            clip_id,
            target_track_id,
            new_in_frame,
            target_index,
        )
    }

    // --- Property / Keyframe Operations ---

    pub fn evaluate_property_value(
        &self,
        property: &crate::model::project::property::Property,
        context: &crate::model::project::property::PropertyMap,
        time: f64,
        fps: f64,
    ) -> PropertyValue {
        self.project_manager
            .evaluate_property_value(property, context, time, fps)
    }

    pub fn update_clip_time(
        &self,
        clip_id: Uuid,
        in_frame: u64,
        out_frame: u64,
    ) -> Result<(), LibraryError> {
        self.update_clip_property(
            clip_id,
            "in_frame",
            PropertyValue::Number(ordered_float::OrderedFloat(in_frame as f64)),
        )?;
        self.update_clip_property(
            clip_id,
            "out_frame",
            PropertyValue::Number(ordered_float::OrderedFloat(out_frame as f64)),
        )?;
        Ok(())
    }

    pub fn update_clip_source_frames(&self, clip_id: Uuid, frame: i64) -> Result<(), LibraryError> {
        self.update_clip_property(
            clip_id,
            "source_begin_frame",
            PropertyValue::Number(ordered_float::OrderedFloat(frame as f64)),
        )
    }

    pub fn get_inspector_definitions(
        &self,
        comp_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
    ) -> Vec<crate::model::project::property::PropertyDefinition> {
        self.project_manager
            .get_inspector_definitions(comp_id, track_id, clip_id)
    }

    pub fn get_property_definitions(
        &self,
        comp_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
    ) -> Vec<crate::model::project::property::PropertyDefinition> {
        self.get_inspector_definitions(comp_id, track_id, clip_id)
    }

    pub fn update_property_or_keyframe(
        &self,
        clip_id: Uuid,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .update_property_or_keyframe(clip_id, property_key, time, value, easing)
    }

    pub fn set_property_attribute(
        &self,
        clip_id: Uuid,
        target: crate::model::project::property::PropertyTarget,
        property_key: &str,
        attribute_key: &str,
        attribute_value: PropertyValue,
    ) -> Result<(), LibraryError> {
        self.project_manager.set_property_attribute(
            clip_id,
            target,
            property_key,
            attribute_key,
            attribute_value,
        )
    }

    pub fn add_target_keyframe(
        &self,
        clip_id: Uuid,
        target: crate::model::project::property::PropertyTarget,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .add_target_keyframe(clip_id, target, property_key, time, value, easing)
    }

    pub fn update_target_keyframe_by_index(
        &self,
        clip_id: Uuid,
        target: crate::model::project::property::PropertyTarget,
        property_key: &str,
        keyframe_index: usize,
        time: Option<f64>,
        value: Option<PropertyValue>,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.project_manager.update_target_keyframe_by_index(
            clip_id,
            target,
            property_key,
            keyframe_index,
            time,
            value,
            easing,
        )
    }

    pub fn remove_target_keyframe_by_index(
        &self,
        clip_id: Uuid,
        target: crate::model::project::property::PropertyTarget,
        property_key: &str,
        keyframe_index: usize,
    ) -> Result<(), LibraryError> {
        self.project_manager.remove_target_keyframe_by_index(
            clip_id,
            target,
            property_key,
            keyframe_index,
        )
    }

    pub fn update_target_property_or_keyframe(
        &self,
        clip_id: Uuid,
        target: crate::model::project::property::PropertyTarget,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.project_manager.update_target_property_or_keyframe(
            clip_id,
            target,
            property_key,
            time,
            value,
            easing,
        )
    }
}
