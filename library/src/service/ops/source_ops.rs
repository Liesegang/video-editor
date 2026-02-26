use crate::error::LibraryError;
use crate::project::property::PropertyValue;
use crate::project::source::SourceData;
use crate::project::track::TrackData;
use crate::service::editor_service::EditorService;
use uuid::Uuid;

/// Source/Layer factory, track, source CRUD, property, and keyframe operations.
impl EditorService {
    // --- Source Factory Methods ---

    pub fn build_audio_source(
        &self,
        reference_id: Option<Uuid>,
        file_path: &str,
        in_frame: u64,
        out_frame: u64,
        source_begin_frame: i64,
        duration_frame: u64,
        fps: f64,
    ) -> SourceData {
        self.project_manager.build_audio_source(
            reference_id,
            file_path,
            in_frame,
            out_frame,
            source_begin_frame,
            duration_frame,
            fps,
        )
    }

    pub fn build_video_source(
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
    ) -> Result<SourceData, LibraryError> {
        self.project_manager.build_video_source(
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

    pub fn build_image_source(
        &self,
        reference_id: Option<Uuid>,
        file_path: &str,
        in_frame: u64,
        out_frame: u64,
        canvas_width: u32,
        canvas_height: u32,
        fps: f64,
    ) -> Result<SourceData, LibraryError> {
        self.project_manager.build_image_source(
            reference_id,
            file_path,
            in_frame,
            out_frame,
            canvas_width,
            canvas_height,
            fps,
        )
    }

    pub fn build_text_source(
        &self,
        text: &str,
        in_frame: u64,
        out_frame: u64,
        canvas_width: u32,
        canvas_height: u32,
        fps: f64,
    ) -> Result<SourceData, LibraryError> {
        self.project_manager.build_text_source(
            text,
            in_frame,
            out_frame,
            canvas_width,
            canvas_height,
            fps,
        )
    }

    pub fn build_shape_source(
        &self,
        in_frame: u64,
        out_frame: u64,
        canvas_width: u32,
        canvas_height: u32,
        fps: f64,
    ) -> Result<SourceData, LibraryError> {
        self.project_manager.build_shape_source(
            in_frame,
            out_frame,
            canvas_width,
            canvas_height,
            fps,
        )
    }

    pub fn build_sksl_source(
        &self,
        in_frame: u64,
        out_frame: u64,
        canvas_width: u32,
        canvas_height: u32,
        fps: f64,
    ) -> Result<SourceData, LibraryError> {
        self.project_manager.build_sksl_source(
            in_frame,
            out_frame,
            canvas_width,
            canvas_height,
            fps,
        )
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

    // --- Source/Layer Operations ---

    pub fn add_layer_to_track(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        source: SourceData,
        in_frame: u64,
        out_frame: u64,
        insert_index: Option<usize>,
    ) -> Result<Uuid, LibraryError> {
        self.project_manager.add_layer_to_track(
            composition_id,
            track_id,
            source,
            in_frame,
            out_frame,
            insert_index,
        )
    }

    pub fn remove_layer_from_track(
        &self,
        track_id: Uuid,
        source_id: Uuid,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .remove_layer_from_track(track_id, source_id)
    }

    pub fn update_source_property(
        &self,
        source_id: Uuid,
        property_key: &str,
        value: PropertyValue,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .update_source_property(source_id, property_key, value)
    }

    pub fn move_layer_to_track(
        &self,
        composition_id: Uuid,
        source_track_id: Uuid,
        source_id: Uuid,
        target_track_id: Uuid,
        new_in_frame: u64,
    ) -> Result<(), LibraryError> {
        self.project_manager.move_layer_to_track(
            composition_id,
            source_track_id,
            source_id,
            target_track_id,
            new_in_frame,
        )
    }

    pub fn move_layer_to_track_at_index(
        &self,
        composition_id: Uuid,
        source_track_id: Uuid,
        source_id: Uuid,
        target_track_id: Uuid,
        new_in_frame: u64,
        target_index: Option<usize>,
    ) -> Result<(), LibraryError> {
        self.project_manager.move_layer_to_track_at_index(
            composition_id,
            source_track_id,
            source_id,
            target_track_id,
            new_in_frame,
            target_index,
        )
    }

    // --- Property / Keyframe Operations ---

    pub fn evaluate_property_value(
        &self,
        property: &crate::project::property::Property,
        context: &crate::project::property::PropertyMap,
        time: f64,
        fps: f64,
    ) -> PropertyValue {
        self.project_manager
            .evaluate_property_value(property, context, time, fps)
    }

    pub fn update_source_time(
        &self,
        source_id: Uuid,
        in_frame: u64,
        out_frame: u64,
    ) -> Result<(), LibraryError> {
        self.update_source_property(
            source_id,
            "in_frame",
            PropertyValue::Number(ordered_float::OrderedFloat(in_frame as f64)),
        )?;
        self.update_source_property(
            source_id,
            "out_frame",
            PropertyValue::Number(ordered_float::OrderedFloat(out_frame as f64)),
        )?;

        // Sync parent Layer timing
        self.project_manager
            .sync_layer_timing(source_id, in_frame, out_frame);

        Ok(())
    }

    pub fn update_source_begin_frame(
        &self,
        source_id: Uuid,
        frame: i64,
    ) -> Result<(), LibraryError> {
        self.update_source_property(
            source_id,
            "source_begin_frame",
            PropertyValue::Number(ordered_float::OrderedFloat(frame as f64)),
        )
    }

    pub fn get_inspector_definitions(
        &self,
        comp_id: Uuid,
        track_id: Uuid,
        source_id: Uuid,
    ) -> Vec<crate::project::property::PropertyDefinition> {
        self.project_manager
            .get_inspector_definitions(comp_id, track_id, source_id)
    }

    pub fn get_property_definitions(
        &self,
        comp_id: Uuid,
        track_id: Uuid,
        source_id: Uuid,
    ) -> Vec<crate::project::property::PropertyDefinition> {
        self.get_inspector_definitions(comp_id, track_id, source_id)
    }

    pub fn update_property_or_keyframe(
        &self,
        source_id: Uuid,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.project_manager.update_property_or_keyframe(
            source_id,
            property_key,
            time,
            value,
            easing,
        )
    }

    pub fn set_property_attribute(
        &self,
        source_id: Uuid,
        target: crate::project::property::PropertyTarget,
        property_key: &str,
        attribute_key: &str,
        attribute_value: PropertyValue,
    ) -> Result<(), LibraryError> {
        self.project_manager.set_property_attribute(
            source_id,
            target,
            property_key,
            attribute_key,
            attribute_value,
        )
    }

    pub fn add_target_keyframe(
        &self,
        source_id: Uuid,
        target: crate::project::property::PropertyTarget,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.project_manager.add_target_keyframe(
            source_id,
            target,
            property_key,
            time,
            value,
            easing,
        )
    }

    pub fn update_target_keyframe_by_index(
        &self,
        source_id: Uuid,
        target: crate::project::property::PropertyTarget,
        property_key: &str,
        keyframe_index: usize,
        time: Option<f64>,
        value: Option<PropertyValue>,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.project_manager.update_target_keyframe_by_index(
            source_id,
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
        source_id: Uuid,
        target: crate::project::property::PropertyTarget,
        property_key: &str,
        keyframe_index: usize,
    ) -> Result<(), LibraryError> {
        self.project_manager.remove_target_keyframe_by_index(
            source_id,
            target,
            property_key,
            keyframe_index,
        )
    }

    pub fn update_target_property_or_keyframe(
        &self,
        source_id: Uuid,
        target: crate::project::property::PropertyTarget,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.project_manager.update_target_property_or_keyframe(
            source_id,
            target,
            property_key,
            time,
            value,
            easing,
        )
    }
}
