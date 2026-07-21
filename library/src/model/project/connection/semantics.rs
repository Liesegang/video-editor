use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use crate::model::NodeContent;

use super::super::Project;
use super::{ContainerImageSource, PortDataType, PortDirection, PortOwner, ProjectConnection};

/// On-demand stable identities for one container's visual graph, recomputed
/// from authoritative containment, output bindings, ports, and connections.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContainerGraphSemantics {
    explicit_output_node_id: Option<Uuid>,
    explicit_output_is_directly_contained: bool,
    authored_source: Option<PortOwner>,
    reaches_output: HashSet<PortOwner>,
}

impl ContainerGraphSemantics {
    /// The raw Node UUID stored in the container's authored output binding.
    /// This remains distinct from the upstream semantic source and can name a
    /// foreign or missing Node in a malformed, directly loaded Project.
    pub fn explicit_output_node_id(&self) -> Option<Uuid> {
        self.explicit_output_node_id
    }

    /// Whether the raw output binding names a direct Node of this container.
    /// A malformed foreign binding remains observable through
    /// [`Self::explicit_output_node_id`] but is never traversed as content.
    pub fn explicit_output_is_directly_contained(&self) -> bool {
        self.explicit_output_is_directly_contained
    }

    /// The first deterministic visual source upstream of the container output.
    /// Runtime enabled/range/availability state deliberately does not affect it.
    /// Composition Instance Nodes are identity terminals and all declared visual input ports share the
    /// `(connection order, connection UUID)` tie-break until port roles exist.
    pub fn authored_source(&self) -> Option<PortOwner> {
        self.authored_source
    }

    pub fn authored_source_node_id(&self) -> Option<Uuid> {
        match self.authored_source {
            Some(PortOwner::Node(node_id)) => Some(node_id),
            _ => None,
        }
    }

    /// Whether an authored visual path connects `owner` to this result.
    pub fn structurally_reaches_output(&self, owner: PortOwner) -> bool {
        self.reaches_output.contains(&owner)
    }
}

impl Project {
    /// Query the semantic identity and visual membership of a container graph.
    ///
    /// Image and Shape port types define visual flow. Variadic order followed
    /// by persistent connection identity defines deterministic source choice.
    /// Explicit output bindings take precedence over derived child composition.
    /// Disabled Nodes and Clip timing are authored/runtime state orthogonal to
    /// identity, so neither is consulted here.
    pub fn container_graph_semantics(&self, owner: PortOwner) -> ContainerGraphSemantics {
        let explicit_output_node_id = self.container_output_node_id(owner);
        let explicit_output_is_directly_contained = explicit_output_node_id
            .is_some_and(|node_id| self.container_directly_contains_node(owner, node_id));
        let image_sources = self.semantic_container_image_sources(owner);
        let visual_inputs = if image_sources.is_empty() {
            HashMap::new()
        } else {
            self.visual_input_index()
        };

        let authored_source = image_sources.iter().find_map(|source| {
            self.first_authored_visual_source(source.source, &visual_inputs, &mut HashSet::new())
        });
        let mut reaches_output = HashSet::new();
        let mut visited = HashSet::new();
        for source in image_sources {
            self.collect_visual_ancestors(
                source.source,
                &visual_inputs,
                &mut reaches_output,
                &mut visited,
            );
        }

        ContainerGraphSemantics {
            explicit_output_node_id,
            explicit_output_is_directly_contained,
            authored_source,
            reaches_output,
        }
    }

    fn semantic_container_image_sources(&self, owner: PortOwner) -> Vec<ContainerImageSource> {
        let output_node_id = self.container_output_node_id(owner);
        if output_node_id
            .is_some_and(|node_id| !self.container_directly_contains_node(owner, node_id))
        {
            // Preserve malformed authored UUIDs in the public result, but do
            // not cross ownership or reinterpret them as child fallback.
            Vec::new()
        } else {
            self.container_image_sources(owner)
        }
    }

    fn first_authored_visual_source(
        &self,
        owner: PortOwner,
        visual_inputs: &HashMap<PortOwner, Vec<&ProjectConnection>>,
        path: &mut HashSet<PortOwner>,
    ) -> Option<PortOwner> {
        if !path.insert(owner) {
            return None;
        }
        let source = match owner {
            PortOwner::Node(node_id) => {
                if let Some(node) = self.get_node(node_id) {
                    match node.content() {
                        NodeContent::Media(_)
                        | NodeContent::Generator(_)
                        | NodeContent::CompositionInstance(_) => Some(owner),
                        NodeContent::PluginOperation(_)
                        | NodeContent::NativeOperation(_)
                        | NodeContent::Merge => visual_inputs.get(&owner).and_then(|connections| {
                            connections.iter().find_map(|connection| {
                                self.first_authored_visual_source(
                                    connection.from.owner,
                                    visual_inputs,
                                    path,
                                )
                            })
                        }),
                        NodeContent::Value(_) | NodeContent::List(_) => None,
                        NodeContent::SoundMerge => None,
                        NodeContent::SoundAnalysis(_) => None,
                    }
                } else {
                    None
                }
            }
            PortOwner::Composition(_) | PortOwner::Track(_) | PortOwner::Clip(_) => self
                .semantic_container_image_sources(owner)
                .into_iter()
                .find_map(|source| {
                    self.first_authored_visual_source(source.source, visual_inputs, path)
                }),
        };
        path.remove(&owner);
        source
    }

    fn collect_visual_ancestors(
        &self,
        owner: PortOwner,
        visual_inputs: &HashMap<PortOwner, Vec<&ProjectConnection>>,
        reaches_output: &mut HashSet<PortOwner>,
        visited: &mut HashSet<PortOwner>,
    ) {
        if !visited.insert(owner) {
            return;
        }
        reaches_output.insert(owner);
        match owner {
            PortOwner::Node(_) => {
                if let Some(connections) = visual_inputs.get(&owner) {
                    for connection in connections {
                        self.collect_visual_ancestors(
                            connection.from.owner,
                            visual_inputs,
                            reaches_output,
                            visited,
                        );
                    }
                }
            }
            PortOwner::Composition(_) | PortOwner::Track(_) | PortOwner::Clip(_) => {
                for source in self.semantic_container_image_sources(owner) {
                    self.collect_visual_ancestors(
                        source.source,
                        visual_inputs,
                        reaches_output,
                        visited,
                    );
                }
            }
        }
    }

    fn visual_input_index(&self) -> HashMap<PortOwner, Vec<&ProjectConnection>> {
        let mut inputs = HashMap::<PortOwner, Vec<&ProjectConnection>>::new();
        for connection in &self.connections {
            if self.is_visual_connection(connection) {
                inputs
                    .entry(connection.to.owner)
                    .or_default()
                    .push(connection);
            }
        }
        for connections in inputs.values_mut() {
            connections.sort_by_key(|connection| (connection.order, connection.id));
        }
        inputs
    }

    fn is_visual_connection(&self, connection: &ProjectConnection) -> bool {
        let Some(source) = self.port_definition(&connection.from, PortDirection::Output) else {
            return false;
        };
        let Some(target) = self.port_definition(&connection.to, PortDirection::Input) else {
            return false;
        };
        matches!(source.data_type, PortDataType::Image | PortDataType::Shape)
            && source.data_type == target.data_type
    }
}
