use egui::{Pos2, Rect, Vec2};
use library::model::project::Project;
use library::model::property::{
    Property, PropertyDefinition, PropertyUiType, PropertyValue, Vec2 as PropertyVec2,
    Vec3 as PropertyVec3, Vec4 as PropertyVec4,
};
use library::model::Clip;
use library::PropertyOwner;
use ordered_float::OrderedFloat;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum PropertyComponent {
    Scalar,
    X,
    Y,
    Z,
    W,
}

const SCALAR_COMPONENTS: [PropertyComponent; 1] = [PropertyComponent::Scalar];
const VEC2_COMPONENTS: [PropertyComponent; 2] = [PropertyComponent::X, PropertyComponent::Y];
const VEC3_COMPONENTS: [PropertyComponent; 3] = [
    PropertyComponent::X,
    PropertyComponent::Y,
    PropertyComponent::Z,
];
const VEC4_COMPONENTS: [PropertyComponent; 4] = [
    PropertyComponent::X,
    PropertyComponent::Y,
    PropertyComponent::Z,
    PropertyComponent::W,
];

fn components_for_ui_type(ui_type: &PropertyUiType) -> &'static [PropertyComponent] {
    match ui_type {
        PropertyUiType::Float { .. } => &SCALAR_COMPONENTS,
        PropertyUiType::Vec2 { .. } => &VEC2_COMPONENTS,
        PropertyUiType::Vec3 { .. } => &VEC3_COMPONENTS,
        PropertyUiType::Vec4 { .. } => &VEC4_COMPONENTS,
        PropertyUiType::Integer { .. }
        | PropertyUiType::Color
        | PropertyUiType::Text
        | PropertyUiType::MultilineText
        | PropertyUiType::Bool
        | PropertyUiType::Dropdown { .. }
        | PropertyUiType::Font => &[],
    }
}

fn components_for_value(value: &PropertyValue) -> Option<&'static [PropertyComponent]> {
    match value {
        PropertyValue::Number(_) => Some(&SCALAR_COMPONENTS),
        PropertyValue::Vec2(_) => Some(&VEC2_COMPONENTS),
        PropertyValue::Vec3(_) => Some(&VEC3_COMPONENTS),
        PropertyValue::Vec4(_) => Some(&VEC4_COMPONENTS),
        PropertyValue::Integer(_)
        | PropertyValue::String(_)
        | PropertyValue::Boolean(_)
        | PropertyValue::ColorValue(_)
        | PropertyValue::Color(_)
        | PropertyValue::Array(_)
        | PropertyValue::Map(_) => None,
    }
}

/// Resolves the plottable components for one authored property. Canonical UI
/// metadata is authoritative; persisted values are only a fallback for Nodes
/// whose descriptor is unavailable.
pub fn numeric_property_components(
    definition: Option<&PropertyDefinition>,
    property: &Property,
) -> Vec<PropertyComponent> {
    if let Some(definition) = definition {
        return components_for_ui_type(definition.ui_type()).to_vec();
    }

    property
        .value()
        .and_then(components_for_value)
        .or_else(|| {
            property
                .keyframes()
                .first()
                .and_then(|keyframe| components_for_value(&keyframe.value))
        })
        .unwrap_or_default()
        .to_vec()
}

pub fn property_component_value(
    value: &PropertyValue,
    component: PropertyComponent,
) -> Result<f64, String> {
    let component_value = match (value, component) {
        (PropertyValue::Number(value), PropertyComponent::Scalar) => value.into_inner(),
        (PropertyValue::Vec2(value), PropertyComponent::X) => value.x.into_inner(),
        (PropertyValue::Vec2(value), PropertyComponent::Y) => value.y.into_inner(),
        (PropertyValue::Vec3(value), PropertyComponent::X) => value.x.into_inner(),
        (PropertyValue::Vec3(value), PropertyComponent::Y) => value.y.into_inner(),
        (PropertyValue::Vec3(value), PropertyComponent::Z) => value.z.into_inner(),
        (PropertyValue::Vec4(value), PropertyComponent::X) => value.x.into_inner(),
        (PropertyValue::Vec4(value), PropertyComponent::Y) => value.y.into_inner(),
        (PropertyValue::Vec4(value), PropertyComponent::Z) => value.z.into_inner(),
        (PropertyValue::Vec4(value), PropertyComponent::W) => value.w.into_inner(),
        _ => {
            return Err(format!(
                "Graph component {component:?} is incompatible with {value:?}"
            ));
        }
    };
    if component_value.is_finite() {
        Ok(component_value)
    } else {
        Err(format!(
            "Graph component {component:?} has a non-finite value"
        ))
    }
}

pub fn replace_property_component(
    current: &PropertyValue,
    component: PropertyComponent,
    replacement: f64,
) -> Result<PropertyValue, String> {
    if !replacement.is_finite() {
        return Err(format!(
            "Graph component {component:?} cannot be replaced with a non-finite value"
        ));
    }
    let replacement = OrderedFloat(replacement);
    match (current, component) {
        (PropertyValue::Number(_), PropertyComponent::Scalar) => {
            Ok(PropertyValue::Number(replacement))
        }
        (PropertyValue::Vec2(value), PropertyComponent::X) => {
            Ok(PropertyValue::Vec2(PropertyVec2 {
                x: replacement,
                y: value.y,
            }))
        }
        (PropertyValue::Vec2(value), PropertyComponent::Y) => {
            Ok(PropertyValue::Vec2(PropertyVec2 {
                x: value.x,
                y: replacement,
            }))
        }
        (PropertyValue::Vec3(value), PropertyComponent::X) => {
            Ok(PropertyValue::Vec3(PropertyVec3 {
                x: replacement,
                y: value.y,
                z: value.z,
            }))
        }
        (PropertyValue::Vec3(value), PropertyComponent::Y) => {
            Ok(PropertyValue::Vec3(PropertyVec3 {
                x: value.x,
                y: replacement,
                z: value.z,
            }))
        }
        (PropertyValue::Vec3(value), PropertyComponent::Z) => {
            Ok(PropertyValue::Vec3(PropertyVec3 {
                x: value.x,
                y: value.y,
                z: replacement,
            }))
        }
        (PropertyValue::Vec4(value), PropertyComponent::X) => {
            Ok(PropertyValue::Vec4(PropertyVec4 {
                x: replacement,
                y: value.y,
                z: value.z,
                w: value.w,
            }))
        }
        (PropertyValue::Vec4(value), PropertyComponent::Y) => {
            Ok(PropertyValue::Vec4(PropertyVec4 {
                x: value.x,
                y: replacement,
                z: value.z,
                w: value.w,
            }))
        }
        (PropertyValue::Vec4(value), PropertyComponent::Z) => {
            Ok(PropertyValue::Vec4(PropertyVec4 {
                x: value.x,
                y: value.y,
                z: replacement,
                w: value.w,
            }))
        }
        (PropertyValue::Vec4(value), PropertyComponent::W) => {
            Ok(PropertyValue::Vec4(PropertyVec4 {
                x: value.x,
                y: value.y,
                z: value.z,
                w: replacement,
            }))
        }
        _ => Err(format!(
            "Graph component {component:?} is incompatible with {current:?}"
        )),
    }
}

#[derive(Clone, Copy)]
pub struct GraphTransform {
    pub graph_rect: Rect,
    pub pan: Vec2,
    pub zoom_x: f32, // pixels per second
    pub zoom_y: f32, // pixels per unit
}

impl GraphTransform {
    pub fn new(graph_rect: Rect, pan: Vec2, zoom_x: f32, zoom_y: f32) -> Self {
        Self {
            graph_rect,
            pan,
            zoom_x,
            zoom_y,
        }
    }

    pub fn to_screen(self, time: f64, value: f64) -> Pos2 {
        let x = self.graph_rect.min.x + self.pan.x + (time as f32 * self.zoom_x);
        let zero_y = self.graph_rect.center().y + self.pan.y;
        let y = zero_y - (value as f32 * self.zoom_y);
        Pos2::new(x, y)
    }

    pub fn screen_to_graph(self, pos: Pos2) -> (f64, f64) {
        let x = pos.x;
        let time = (x - self.graph_rect.min.x - self.pan.x) / self.zoom_x;
        let zero_y = self.graph_rect.center().y + self.pan.y;
        let y = pos.y;
        let value = (zero_y - y) / self.zoom_y;
        (time as f64, value as f64)
    }
}

#[derive(Clone, Copy)]
pub struct TimeMapper {
    pub clip_start_time: f64,
    pub trim_in: f64,
    pub time_stretch: f64,
}

impl TimeMapper {
    pub const fn identity() -> Self {
        Self {
            clip_start_time: 0.0,
            trim_in: 0.0,
            time_stretch: 1.0,
        }
    }

    pub fn from_clip(clip: &Clip) -> Self {
        Self {
            clip_start_time: clip.start_time.into_inner(),
            trim_in: clip.trim_in.into_inner(),
            time_stretch: clip.time_stretch.into_inner(),
        }
    }

    pub fn to_source_time(self, global_time: f64) -> f64 {
        self.trim_in + (global_time - self.clip_start_time) * self.time_stretch
    }

    pub fn to_global_time(self, source_time: f64) -> f64 {
        if self.time_stretch.abs() <= f64::EPSILON {
            self.clip_start_time
        } else {
            self.clip_start_time + (source_time - self.trim_in) / self.time_stretch
        }
    }
}

pub fn time_mapper_for_owner(project: &Project, owner: PropertyOwner) -> TimeMapper {
    let clip = match owner {
        PropertyOwner::Clip(clip_id) => project.get_clip(clip_id),
        PropertyOwner::Node(node_id) => project
            .find_parent_clip(node_id)
            .and_then(|clip_id| project.get_clip(clip_id)),
    };
    clip.map_or_else(TimeMapper::identity, TimeMapper::from_clip)
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::animation::EasingFunction;
    use library::model::property::{Keyframe, Vec3, Vec4};
    use ordered_float::OrderedFloat;

    fn vec3(x: f64, y: f64, z: f64) -> PropertyValue {
        PropertyValue::Vec3(Vec3 {
            x: OrderedFloat(x),
            y: OrderedFloat(y),
            z: OrderedFloat(z),
        })
    }

    fn vec4(x: f64, y: f64, z: f64, w: f64) -> PropertyValue {
        PropertyValue::Vec4(Vec4 {
            x: OrderedFloat(x),
            y: OrderedFloat(y),
            z: OrderedFloat(z),
            w: OrderedFloat(w),
        })
    }

    #[test]
    fn numeric_components_are_definition_first_then_expression_or_keyframe_fallback() {
        let definition = PropertyDefinition::new(
            "vector",
            PropertyUiType::vec4(""),
            "Vector",
            vec4(0.0, 0.0, 0.0, 0.0),
        );
        let scalar_expression = Property::expression(
            "value + time".to_string(),
            PropertyValue::Number(OrderedFloat(2.0)),
        );
        assert_eq!(
            numeric_property_components(Some(&definition), &scalar_expression),
            VEC4_COMPONENTS
        );

        let vector_expression = Property::expression("value".to_string(), vec3(1.0, 2.0, 3.0));
        assert_eq!(
            numeric_property_components(None, &vector_expression),
            VEC3_COMPONENTS
        );

        let mut keyframed = Property::keyframe(vec![Keyframe::new(
            0.0,
            vec4(4.0, 3.0, 2.0, 1.0),
            EasingFunction::Linear,
        )]);
        assert!(keyframed.properties.remove("value").is_some());
        assert_eq!(
            numeric_property_components(None, &keyframed),
            VEC4_COMPONENTS
        );
    }

    #[test]
    fn vec4_w_replacement_preserves_xyz_and_wrong_components_are_rejected() {
        let original = vec4(1.0, 2.0, 3.0, 4.0);
        assert_eq!(
            replace_property_component(&original, PropertyComponent::W, 9.0),
            Ok(vec4(1.0, 2.0, 3.0, 9.0))
        );
        assert_eq!(
            property_component_value(&original, PropertyComponent::W),
            Ok(4.0)
        );
        assert!(
            replace_property_component(&vec3(1.0, 2.0, 3.0), PropertyComponent::W, 9.0).is_err()
        );
        assert!(property_component_value(&original, PropertyComponent::Scalar).is_err());
    }

    #[test]
    fn clip_time_mapping_is_exact_with_fractional_start_trim_and_stretch() {
        let mut clip = Clip::new("mapped", 1.125, 5.0);
        clip.trim_in = OrderedFloat(0.375);
        clip.time_stretch = OrderedFloat(1.5);
        let mapper = TimeMapper::from_clip(&clip);

        let global = 2.625;
        let source = mapper.to_source_time(global);
        assert!((source - 2.625).abs() < f64::EPSILON);
        assert!((mapper.to_global_time(source) - global).abs() < f64::EPSILON);
    }

    #[test]
    fn zero_stretch_maps_every_source_time_to_the_clip_start() {
        let mut clip = Clip::new("frozen", 3.25, 5.0);
        clip.trim_in = OrderedFloat(1.75);
        clip.time_stretch = OrderedFloat(0.0);
        let mapper = TimeMapper::from_clip(&clip);

        assert_eq!(mapper.to_source_time(99.0), 1.75);
        assert_eq!(mapper.to_global_time(123.0), 3.25);
    }

    #[test]
    fn same_uuid_clip_does_not_hijack_node_time_scope() {
        let shared_id = uuid::Uuid::new_v4();
        let parent_clip_id = uuid::Uuid::new_v4();
        let mut project = Project::new("typed graph time scope");

        let mut colliding_clip = Clip::new("same UUID Clip", 100.0, 5.0);
        colliding_clip.id = shared_id;
        colliding_clip.trim_in = OrderedFloat(20.0);
        let mut parent_clip = Clip::new("actual Node parent", 2.0, 5.0);
        parent_clip.id = parent_clip_id;
        parent_clip.trim_in = OrderedFloat(0.5);
        let mut node = library::model::Node::new_merge("same UUID Node");
        node.id = shared_id;
        parent_clip.node_ids.push(shared_id);

        project.add_clip(colliding_clip);
        project.add_clip(parent_clip);
        project.add_node(node);

        let node_mapper = time_mapper_for_owner(&project, PropertyOwner::Node(shared_id));
        let clip_mapper = time_mapper_for_owner(&project, PropertyOwner::Clip(shared_id));

        assert_eq!(node_mapper.clip_start_time, 2.0);
        assert_eq!(node_mapper.trim_in, 0.5);
        assert_eq!(clip_mapper.clip_start_time, 100.0);
        assert_eq!(clip_mapper.trim_in, 20.0);
    }
}
