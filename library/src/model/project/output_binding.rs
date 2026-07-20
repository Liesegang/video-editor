use super::*;

impl Project {
    /// Select the directly contained Node that provides this container's
    /// typed Image output.
    pub fn set_output_node(
        &mut self,
        container: NodeContainer,
        output_node_id: Option<Uuid>,
    ) -> Result<(), ProjectGraphError> {
        let previous_output_node_id = match container {
            NodeContainer::Composition(id) => {
                self.get_composition(id)
                    .ok_or(ProjectGraphError::CompositionNotFound(id))?
                    .output_node_id
            }
            NodeContainer::Track(id) => {
                self.get_track(id)
                    .ok_or(ProjectGraphError::TrackNotFound(id))?
                    .output_node_id
            }
            NodeContainer::Clip(id) => {
                self.get_clip(id)
                    .ok_or(ProjectGraphError::ClipNotFound(id))?
                    .output_node_id
            }
        };
        if let Some(node_id) = output_node_id
            && self.find_node_container(node_id) != Some(container)
        {
            return Err(ProjectGraphError::OutputNodeOutsideContainer { node_id, container });
        }
        if let Some(node_id) = output_node_id {
            let image_output = PortAddress::new(PortOwner::Node(node_id), IMAGE_OUTPUT_PORT);
            if !self
                .port_definition(&image_output, PortDirection::Output)
                .is_some_and(|port| port.data_type == PortDataType::Image)
            {
                return Err(ProjectGraphError::OutputNodeHasNoImagePort { node_id, container });
            }
        }

        let validation_baseline = self.validate_connections();
        self.set_container_output_node_unchecked(container, output_node_id);
        if let Some(error) =
            first_new_project_validation_error(&validation_baseline, self.validate_connections())
        {
            self.set_container_output_node_unchecked(container, previous_output_node_id);
            return Err(error);
        }
        Ok(())
    }

    /// Select the directly contained Node that provides this container's
    /// typed Audio output. This binding is independent from the Image output
    /// selected by [`Self::set_output_node`].
    pub fn set_audio_output_node(
        &mut self,
        container: NodeContainer,
        output_node_id: Option<Uuid>,
    ) -> Result<(), ProjectGraphError> {
        let previous_output_node_id = match container {
            NodeContainer::Composition(id) => {
                self.get_composition(id)
                    .ok_or(ProjectGraphError::CompositionNotFound(id))?
                    .audio_output_node_id
            }
            NodeContainer::Track(id) => {
                self.get_track(id)
                    .ok_or(ProjectGraphError::TrackNotFound(id))?
                    .audio_output_node_id
            }
            NodeContainer::Clip(id) => {
                self.get_clip(id)
                    .ok_or(ProjectGraphError::ClipNotFound(id))?
                    .audio_output_node_id
            }
        };
        if let Some(node_id) = output_node_id
            && self.find_node_container(node_id) != Some(container)
        {
            return Err(ProjectGraphError::OutputNodeOutsideContainer { node_id, container });
        }
        if let Some(node_id) = output_node_id {
            let audio_output = PortAddress::new(PortOwner::Node(node_id), AUDIO_OUTPUT_PORT);
            if !self
                .port_definition(&audio_output, PortDirection::Output)
                .is_some_and(|port| port.data_type == PortDataType::Audio)
            {
                return Err(ProjectGraphError::OutputNodeHasNoAudioPort { node_id, container });
            }
        }

        let validation_baseline = self.validate_connections();
        self.set_container_audio_output_node_unchecked(container, output_node_id);
        if let Some(error) =
            first_new_project_validation_error(&validation_baseline, self.validate_connections())
        {
            self.set_container_audio_output_node_unchecked(container, previous_output_node_id);
            return Err(error);
        }
        Ok(())
    }

    pub(super) fn set_container_output_node_unchecked(
        &mut self,
        container: NodeContainer,
        output_node_id: Option<Uuid>,
    ) {
        match container {
            NodeContainer::Composition(id) => {
                if let Some(composition) = self.get_composition_mut(id) {
                    composition.output_node_id = output_node_id;
                }
            }
            NodeContainer::Track(id) => {
                if let Some(track) = self.get_track_mut(id) {
                    track.output_node_id = output_node_id;
                }
            }
            NodeContainer::Clip(id) => {
                if let Some(clip) = self.get_clip_mut(id) {
                    clip.output_node_id = output_node_id;
                }
            }
        }
    }

    pub(super) fn set_container_audio_output_node_unchecked(
        &mut self,
        container: NodeContainer,
        output_node_id: Option<Uuid>,
    ) {
        match container {
            NodeContainer::Composition(id) => {
                if let Some(composition) = self.get_composition_mut(id) {
                    composition.audio_output_node_id = output_node_id;
                }
            }
            NodeContainer::Track(id) => {
                if let Some(track) = self.get_track_mut(id) {
                    track.audio_output_node_id = output_node_id;
                }
            }
            NodeContainer::Clip(id) => {
                if let Some(clip) = self.get_clip_mut(id) {
                    clip.audio_output_node_id = output_node_id;
                }
            }
        }
    }
}
