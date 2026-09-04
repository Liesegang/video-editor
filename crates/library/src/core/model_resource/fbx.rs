mod support;

use std::collections::HashMap;
use std::mem::size_of;

use super::service::{ModelDecodeLimits, enforce_limit};
use super::{
    EmbeddedModelTexture, MeshMaterial, MeshPrimitive, MeshScene, MeshSceneNode, MeshVertex,
    ModelDiagnostic, ModelDiagnosticCode, ModelResourceError, ModelResourceKey, ModelSourceFormat,
    ModelSourceMetadata, StaticTriangleMesh,
};
use support::{
    OwnedBudget, WorkingBudget, bounded_element_name, bounded_text, checked_color, checked_matrix,
    checked_product, checked_scalar, checked_vec2, checked_vec3, copy_owned_text, copy_text,
    generate_vertex_normals, validate_vec2_attribute, validate_vec3_attribute,
};

pub(super) fn decode_fbx(
    bytes: &[u8],
    key: ModelResourceKey,
    limits: &ModelDecodeLimits,
) -> Result<MeshScene, ModelResourceError> {
    let deny_external = |_path: &str, _info: &ufbx::OpenFileInfo| None;
    let mut options = ufbx::LoadOpts::default();
    let parser_budget = limits.max_parser_bytes / 3;
    options.temp_allocator.memory_limit = parser_budget;
    options.result_allocator.memory_limit = parser_budget;
    options.thread_opts.memory_limit = parser_budget;
    options.load_external_files = false;
    options.ignore_missing_external_files = true;
    options.open_file_cb = ufbx::OpenFileCb::Ref(&deny_external);
    options.evaluate_skinning = false;
    options.evaluate_caches = false;
    options.skip_skin_vertices = true;
    options.generate_missing_normals = key.normalization.generate_missing_normals;
    options.normalize_normals = true;
    options.strict = true;
    options.force_single_thread_ascii_parsing = true;
    options.node_depth_limit = u32::try_from(limits.max_hierarchy_depth).map_err(|_| {
        ModelResourceError::InvalidLimits {
            detail: "hierarchy depth exceeds u32".to_string(),
        }
    })?;
    options.file_size_estimate = bytes.len() as u64;
    options.file_format = ufbx::FileFormat::Fbx;
    options.no_format_from_extension = true;
    options.target_axes = ufbx::CoordinateAxes::right_handed_y_up();
    options.target_unit_meters = 1.0;
    options.space_conversion = ufbx::SpaceConversion::AdjustTransforms;
    options.geometry_transform_handling = ufbx::GeometryTransformHandling::HelperNodes;
    options.inherit_mode_handling = ufbx::InheritModeHandling::HelperNodes;

    let parser_scene =
        ufbx::load_memory(bytes, options).map_err(|error| ModelResourceError::Decode {
            detail: format!("{error:?}"),
        })?;
    validate_scene_counts(&parser_scene, limits)?;

    let mut budget = OwnedBudget::new(limits.max_scene_bytes);
    let mut working = WorkingBudget::new(limits.max_working_bytes);
    let generate_missing_normals = key.normalization.generate_missing_normals;
    budget.reserve("owned scene bytes", size_of::<MeshScene>())?;
    let mut diagnostics = collect_diagnostics(&parser_scene);
    let (textures, texture_indices) = copy_textures(
        &parser_scene,
        limits,
        &mut budget,
        &mut working,
        &mut diagnostics,
    )?;
    let (materials, material_indices) = copy_materials(
        &parser_scene,
        &texture_indices,
        &mut budget,
        &mut working,
        &mut diagnostics,
    )?;
    let (meshes, mesh_indices) = copy_meshes(
        &parser_scene,
        limits,
        &mut budget,
        &mut working,
        &mut diagnostics,
        generate_missing_normals,
    )?;
    let nodes = copy_nodes(
        &parser_scene,
        &mesh_indices,
        &meshes,
        &material_indices,
        limits,
        &mut budget,
        &mut working,
    )?;

    if meshes.iter().all(|mesh| mesh.indices.is_empty()) {
        return Err(ModelResourceError::NoRenderableGeometry { diagnostics });
    }

    let creator = copy_text(&parser_scene.metadata.creator, &mut budget)?;
    let original_unit_meters = parser_scene.settings.original_unit_meters;
    if !original_unit_meters.is_finite() || original_unit_meters <= 0.0 {
        return Err(ModelResourceError::InvalidData {
            detail: "FBX original unit scale is non-finite or non-positive".to_string(),
        });
    }
    Ok(MeshScene {
        key,
        nodes,
        meshes,
        materials,
        textures,
        diagnostics,
        source_metadata: ModelSourceMetadata {
            format: ModelSourceFormat::Fbx,
            source_version: parser_scene.metadata.version,
            creator,
            original_unit_meters,
        },
    })
}

fn validate_scene_counts(
    scene: &ufbx::Scene,
    limits: &ModelDecodeLimits,
) -> Result<(), ModelResourceError> {
    enforce_limit("nodes", scene.nodes.count, limits.max_nodes)?;
    enforce_limit("meshes", scene.meshes.count, limits.max_meshes)?;
    enforce_limit("materials", scene.materials.count, limits.max_materials)?;
    enforce_limit("textures", scene.textures.count, limits.max_textures)?;

    let mut vertices = 0_usize;
    let mut indices = 0_usize;
    let mut faces = 0_usize;
    for mesh in &scene.meshes {
        vertices = checked_sum(vertices, mesh.num_indices, "vertices", limits.max_vertices)?;
        let mesh_indices = mesh.num_triangles.saturating_mul(3);
        indices = checked_sum(indices, mesh_indices, "indices", limits.max_indices)?;
        faces = checked_sum(faces, mesh.faces.count, "faces", limits.max_faces)?;
    }
    Ok(())
}

fn checked_sum(
    current: usize,
    value: usize,
    resource: &'static str,
    limit: usize,
) -> Result<usize, ModelResourceError> {
    let total = current.saturating_add(value);
    enforce_limit(resource, total, limit)?;
    Ok(total)
}

fn collect_diagnostics(scene: &ufbx::Scene) -> Vec<ModelDiagnostic> {
    let mut diagnostics = Vec::new();
    for warning in &scene.metadata.warnings {
        diagnostics.push(ModelDiagnostic::warning(
            ModelDiagnosticCode::ParserWarning,
            format!(
                "FBX parser warning {:?} ({} occurrence(s)): {}",
                warning.type_,
                warning.count,
                bounded_text(&warning.description, 1_024)
            ),
            None,
        ));
    }
    if scene.anim_stacks.count > 0 || scene.anim_layers.count > 0 || scene.anim_curves.count > 0 {
        diagnostics.push(ModelDiagnostic::warning(
            ModelDiagnosticCode::AnimationUnsupported,
            format!(
                "Animation is present ({} stack(s), {} layer(s), {} curve(s)); the static-mesh slice uses the bind-time scene",
                scene.anim_stacks.count, scene.anim_layers.count, scene.anim_curves.count
            ),
            None,
        ));
    }
    if scene.cameras.count > 0 {
        diagnostics.push(ModelDiagnostic::warning(
            ModelDiagnosticCode::CameraUnsupported,
            format!(
                "{} embedded camera(s) were retained only as hierarchy nodes",
                scene.cameras.count
            ),
            None,
        ));
    }
    if scene.lights.count > 0 {
        diagnostics.push(ModelDiagnostic::warning(
            ModelDiagnosticCode::LightUnsupported,
            format!(
                "{} embedded light(s) were retained only as hierarchy nodes",
                scene.lights.count
            ),
            None,
        ));
    }
    let other_geometry_count = scene.line_curves.count
        + scene.nurbs_curves.count
        + scene.nurbs_surfaces.count
        + scene.nurbs_trim_surfaces.count
        + scene.procedural_geometries.count;
    if other_geometry_count > 0 {
        diagnostics.push(ModelDiagnostic::warning(
            ModelDiagnosticCode::UnsupportedGeometry,
            format!(
                "{other_geometry_count} non-polygon geometry element(s) are outside the static triangle-mesh slice"
            ),
            None,
        ));
    }
    diagnostics
}

fn copy_textures(
    scene: &ufbx::Scene,
    limits: &ModelDecodeLimits,
    budget: &mut OwnedBudget,
    working: &mut WorkingBudget,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> Result<(Vec<EmbeddedModelTexture>, HashMap<u32, usize>), ModelResourceError> {
    let mut textures = budget.vec_with_capacity::<EmbeddedModelTexture>(scene.textures.count)?;
    let mut indices = working.hash_map_with_capacity(scene.textures.count)?;
    let mut embedded_bytes = 0_usize;
    for texture in &scene.textures {
        diagnose_texture_semantics(texture, diagnostics);
        let content: &[u8] = if !texture.content.is_empty() {
            &texture.content
        } else if let Some(video) = texture.video.as_ref() {
            &video.content
        } else {
            &[]
        };
        let name = bounded_element_name(&texture.element.name);
        if content.is_empty() {
            if texture.has_file
                || !texture.filename.is_empty()
                || !texture.relative_filename.is_empty()
            {
                diagnostics.push(ModelDiagnostic::warning(
                    ModelDiagnosticCode::ExternalTextureNotLoaded,
                    "External texture was not loaded; automatic external file access is disabled",
                    Some(name),
                ));
            }
            continue;
        }
        embedded_bytes = checked_sum(
            embedded_bytes,
            content.len(),
            "embedded texture bytes",
            limits.max_embedded_texture_bytes,
        )?;
        budget.reserve("owned scene bytes", content.len())?;
        let mut encoded_bytes = Vec::new();
        encoded_bytes
            .try_reserve_exact(content.len())
            .map_err(|_| ModelResourceError::AllocationFailed {
                resource: "embedded texture bytes",
                requested: content.len(),
            })?;
        encoded_bytes.extend_from_slice(content);
        let output_index = textures.len();
        indices.insert(texture.element.element_id, output_index);
        textures.push(EmbeddedModelTexture {
            name: copy_owned_text(&name, budget)?,
            encoded_bytes,
        });
    }
    Ok((textures, indices))
}

fn diagnose_texture_semantics(texture: &ufbx::Texture, diagnostics: &mut Vec<ModelDiagnostic>) {
    let name = bounded_element_name(&texture.element.name);
    if texture.type_ == ufbx::TextureType::Layered
        || !texture.layers.is_empty()
        || texture.file_textures.count > 1
    {
        diagnostics.push(ModelDiagnostic::warning(
            ModelDiagnosticCode::LayeredTextureUnsupported,
            "Layered texture composition is not represented; no layer is selected implicitly",
            Some(name.clone()),
        ));
    }
    if texture.type_ == ufbx::TextureType::Procedural {
        diagnostics.push(ModelDiagnostic::warning(
            ModelDiagnosticCode::ProceduralTextureUnsupported,
            "Procedural texture evaluation is not represented",
            Some(name.clone()),
        ));
    }
    if texture.type_ == ufbx::TextureType::Shader || texture.shader.is_some() {
        diagnostics.push(ModelDiagnostic::warning(
            ModelDiagnosticCode::ShaderTextureUnsupported,
            "Shader texture evaluation and output selection are not represented",
            Some(name.clone()),
        ));
    }
    if texture.has_uv_transform {
        diagnostics.push(ModelDiagnostic::warning(
            ModelDiagnosticCode::TextureUvTransformUnsupported,
            "Texture UV transform is present but is not represented",
            Some(name.clone()),
        ));
    }
    if !texture.uv_set.is_empty() {
        diagnostics.push(ModelDiagnostic::warning(
            ModelDiagnosticCode::TextureUvSetSelectionUnsupported,
            format!(
                "Explicit texture UV set {:?} is not retained; the static slice uses UV0",
                bounded_text(&texture.uv_set, 512)
            ),
            Some(name.clone()),
        ));
    }
    if texture.wrap_u != ufbx::WrapMode::Repeat || texture.wrap_v != ufbx::WrapMode::Repeat {
        diagnostics.push(ModelDiagnostic::warning(
            ModelDiagnosticCode::TextureWrapModeUnsupported,
            format!(
                "Texture wrap modes {:?}/{:?} are not represented",
                texture.wrap_u, texture.wrap_v
            ),
            Some(name),
        ));
    }
}

fn copy_materials(
    scene: &ufbx::Scene,
    texture_indices: &HashMap<u32, usize>,
    budget: &mut OwnedBudget,
    working: &mut WorkingBudget,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> Result<(Vec<MeshMaterial>, HashMap<u32, usize>), ModelResourceError> {
    let mut materials = budget.vec_with_capacity::<MeshMaterial>(scene.materials.count)?;
    let mut indices = working.hash_map_with_capacity(scene.materials.count)?;
    for material in &scene.materials {
        let name = bounded_element_name(&material.element.name);
        let (base_map, factor_map) = if material.pbr.base_color.has_value {
            (&material.pbr.base_color, Some(&material.pbr.base_factor))
        } else {
            (
                &material.fbx.diffuse_color,
                Some(&material.fbx.diffuse_factor),
            )
        };
        let mut base_color = if base_map.has_value {
            checked_color(base_map.value_vec4, &name)?
        } else {
            [0.8, 0.8, 0.8, 1.0]
        };
        if let Some(factor) = factor_map.filter(|factor| factor.has_value) {
            let value = checked_scalar(factor.value_vec4.x, "material base factor", &name)?;
            for channel in &mut base_color[..3] {
                *channel = checked_product(*channel, value, "material base color", &name)?;
            }
        }
        if material.pbr.opacity.has_value {
            base_color[3] =
                checked_scalar(material.pbr.opacity.value_vec4.x, "material opacity", &name)?;
        }
        let base_texture_ref = base_map
            .texture
            .as_ref()
            .filter(|_| base_map.texture_enabled)
            .map(std::ops::Deref::deref);
        let base_color_texture = base_texture_ref
            .and_then(|texture| texture_indices.get(&texture.element.element_id).copied());
        let supported_texture_count = usize::from(base_texture_ref.is_some());
        let advanced_features = material.features.coat.enabled
            || material.features.sheen.enabled
            || material.features.transmission.enabled
            || material.features.emission.enabled
            || material.features.matte.enabled
            || material.textures.count > supported_texture_count;
        if advanced_features {
            diagnostics.push(ModelDiagnostic::warning(
                ModelDiagnosticCode::AdvancedMaterialUnsupported,
                "Only base color and one embedded base-color texture are represented",
                Some(name.clone()),
            ));
        }
        let output_index = materials.len();
        indices.insert(material.element.element_id, output_index);
        materials.push(MeshMaterial {
            name: copy_owned_text(&name, budget)?,
            base_color,
            base_color_texture,
        });
    }
    Ok((materials, indices))
}

fn copy_meshes(
    scene: &ufbx::Scene,
    limits: &ModelDecodeLimits,
    budget: &mut OwnedBudget,
    working: &mut WorkingBudget,
    diagnostics: &mut Vec<ModelDiagnostic>,
    generate_missing_normals: bool,
) -> Result<(Vec<StaticTriangleMesh>, HashMap<u32, usize>), ModelResourceError> {
    let mut meshes = budget.vec_with_capacity::<StaticTriangleMesh>(scene.meshes.count)?;
    let mut indices = working.hash_map_with_capacity(scene.meshes.count)?;
    for mesh in &scene.meshes {
        let name = bounded_element_name(&mesh.element.name);
        if mesh.skin_deformers.count > 0 {
            diagnostics.push(ModelDiagnostic::warning(
                ModelDiagnosticCode::SkinningUnsupported,
                format!(
                    "{} skin deformer(s) are ignored; static bind-time geometry is used",
                    mesh.skin_deformers.count
                ),
                Some(name.clone()),
            ));
        }
        if mesh.blend_deformers.count > 0 {
            diagnostics.push(ModelDiagnostic::warning(
                ModelDiagnosticCode::MorphTargetsUnsupported,
                format!(
                    "{} morph/blend deformer(s) are ignored",
                    mesh.blend_deformers.count
                ),
                Some(name.clone()),
            ));
        }
        if mesh.cache_deformers.count > 0 {
            diagnostics.push(ModelDiagnostic::warning(
                ModelDiagnosticCode::GeometryCacheUnsupported,
                format!(
                    "{} geometry cache deformer(s) are ignored",
                    mesh.cache_deformers.count
                ),
                Some(name.clone()),
            ));
        }
        if mesh.uv_sets.count > 1 {
            diagnostics.push(ModelDiagnostic::warning(
                ModelDiagnosticCode::AdditionalUvSetsUnsupported,
                format!(
                    "{} UV sets are present; only UV set 0 is represented",
                    mesh.uv_sets.count
                ),
                Some(name.clone()),
            ));
        }
        let output_index = meshes.len();
        indices.insert(mesh.element.element_id, output_index);
        meshes.push(copy_mesh(
            mesh,
            limits,
            budget,
            working,
            diagnostics,
            generate_missing_normals,
        )?);
    }
    Ok((meshes, indices))
}

fn copy_mesh(
    mesh: &ufbx::Mesh,
    limits: &ModelDecodeLimits,
    budget: &mut OwnedBudget,
    working: &mut WorkingBudget,
    diagnostics: &mut Vec<ModelDiagnostic>,
    generate_missing_normals: bool,
) -> Result<StaticTriangleMesh, ModelResourceError> {
    let name = bounded_element_name(&mesh.element.name);
    validate_vec3_attribute(
        &mesh.vertex_position,
        mesh.num_indices,
        "position",
        true,
        &name,
    )?;
    validate_vec3_attribute(
        &mesh.vertex_normal,
        mesh.num_indices,
        "normal",
        false,
        &name,
    )?;
    validate_vec2_attribute(&mesh.vertex_uv, mesh.num_indices, "UV0", false, &name)?;

    let has_normals = mesh.vertex_normal.exists;
    let has_source_normals = has_normals && !mesh.generated_normals;
    let has_uv0 = mesh.vertex_uv.exists;
    let mut vertices = budget.vec_with_capacity::<MeshVertex>(mesh.num_indices)?;
    for vertex_index in 0..mesh.num_indices {
        vertices.push(MeshVertex {
            position: checked_vec3(mesh.vertex_position[vertex_index], "position", &name)?,
            normal: if has_normals {
                checked_vec3(mesh.vertex_normal[vertex_index], "normal", &name)?
            } else {
                [0.0; 3]
            },
            uv0: if has_uv0 {
                checked_vec2(mesh.vertex_uv[vertex_index], "UV0", &name)?
            } else {
                [0.0; 2]
            },
        });
    }

    let output_capacity =
        mesh.num_triangles
            .checked_mul(3)
            .ok_or_else(|| ModelResourceError::InvalidData {
                detail: format!("mesh {name:?} triangle index count overflows"),
            })?;
    enforce_limit("indices", output_capacity, limits.max_indices)?;
    let mut output_indices = budget.vec_with_capacity::<u32>(output_capacity)?;
    let mut primitives = budget.vec_with_capacity::<MeshPrimitive>(mesh.faces.count)?;
    let max_face_indices = mesh
        .faces
        .iter()
        .map(|face| {
            (face.num_indices as usize)
                .saturating_sub(2)
                .saturating_mul(3)
        })
        .max()
        .unwrap_or(0);
    enforce_limit("indices", max_face_indices, limits.max_indices)?;
    let mut face_indices =
        working.vec_with_capacity::<u32>(max_face_indices, "decode working bytes")?;
    for (face_index, face) in mesh.faces.iter().copied().enumerate() {
        let begin = face.index_begin as usize;
        let count = face.num_indices as usize;
        let end = begin
            .checked_add(count)
            .ok_or_else(|| ModelResourceError::InvalidData {
                detail: format!("mesh {name:?} face {face_index} index range overflows"),
            })?;
        if end > mesh.num_indices {
            return Err(ModelResourceError::InvalidData {
                detail: format!(
                    "mesh {name:?} face {face_index} references polygon vertices {begin}..{end}, but only {} exist",
                    mesh.num_indices
                ),
            });
        }
        if !mesh.face_hole.is_empty()
            && *mesh
                .face_hole
                .get(face_index)
                .ok_or_else(|| ModelResourceError::InvalidData {
                    detail: format!("mesh {name:?} has an incomplete face-hole array"),
                })?
        {
            continue;
        }
        if count < 3 {
            continue;
        }
        let triangle_capacity = count
            .checked_sub(2)
            .and_then(|value| value.checked_mul(3))
            .ok_or_else(|| ModelResourceError::InvalidData {
                detail: format!("mesh {name:?} face {face_index} triangle count overflows"),
            })?;
        face_indices.resize(triangle_capacity, 0);
        let triangle_count = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ufbx::triangulate_face(&mut face_indices, mesh, face) as usize
        }))
        .map_err(|_| ModelResourceError::InvalidData {
            detail: format!("mesh {name:?} face {face_index} triangulation failed"),
        })?;
        let written =
            triangle_count
                .checked_mul(3)
                .ok_or_else(|| ModelResourceError::InvalidData {
                    detail: format!("mesh {name:?} face {face_index} triangulation overflow"),
                })?;
        if written > face_indices.len() {
            return Err(ModelResourceError::InvalidData {
                detail: format!("mesh {name:?} face {face_index} triangulator exceeded its buffer"),
            });
        }
        if let Some(index) = face_indices[..written]
            .iter()
            .find(|index| **index as usize >= mesh.num_indices)
        {
            return Err(ModelResourceError::InvalidData {
                detail: format!(
                    "mesh {name:?} triangulation produced out-of-range polygon vertex {index}"
                ),
            });
        }
        let material_slot = face_material_slot(mesh, face_index, &name)?;
        let first_index =
            u32::try_from(output_indices.len()).map_err(|_| ModelResourceError::InvalidData {
                detail: format!("mesh {name:?} index offset exceeds u32"),
            })?;
        let index_count = u32::try_from(written).map_err(|_| ModelResourceError::InvalidData {
            detail: format!("mesh {name:?} primitive index count exceeds u32"),
        })?;
        let next_len = output_indices.len().checked_add(written).ok_or_else(|| {
            ModelResourceError::InvalidData {
                detail: format!("mesh {name:?} output index count overflows"),
            }
        })?;
        if next_len > output_capacity {
            return Err(ModelResourceError::InvalidData {
                detail: format!(
                    "mesh {name:?} triangulation produced {next_len} indices, exceeding its declared {output_capacity}"
                ),
            });
        }
        output_indices.extend_from_slice(&face_indices[..written]);
        if let Some(previous) = primitives.last_mut().filter(|primitive| {
            primitive.material_slot == material_slot
                && primitive.first_index.checked_add(primitive.index_count) == Some(first_index)
        }) {
            previous.index_count =
                previous
                    .index_count
                    .checked_add(index_count)
                    .ok_or_else(|| ModelResourceError::InvalidData {
                        detail: format!("mesh {name:?} primitive index count overflows"),
                    })?;
        } else {
            primitives.push(MeshPrimitive {
                first_index,
                index_count,
                material_slot,
            });
        }
    }
    if generate_missing_normals && !has_normals && !output_indices.is_empty() {
        let degenerate = generate_vertex_normals(&mut vertices, &output_indices, working)?;
        if degenerate > 0 {
            diagnostics.push(ModelDiagnostic::warning(
                ModelDiagnosticCode::DegenerateNormalGenerated,
                format!(
                    "{degenerate} polygon vertex normal(s) used a fallback because adjoining triangles were degenerate"
                ),
                Some(name.clone()),
            ));
        }
    }
    Ok(StaticTriangleMesh {
        name: copy_owned_text(&name, budget)?,
        vertices,
        indices: output_indices,
        primitives,
        source_face_count: mesh.faces.count,
        has_source_normals,
        has_uv0,
    })
}

fn face_material_slot(
    mesh: &ufbx::Mesh,
    face_index: usize,
    mesh_name: &str,
) -> Result<Option<usize>, ModelResourceError> {
    if mesh.face_material.is_empty() {
        return Ok(None);
    }
    let local_index =
        *mesh
            .face_material
            .get(face_index)
            .ok_or_else(|| ModelResourceError::InvalidData {
                detail: format!("mesh {mesh_name:?} has an incomplete face-material array"),
            })?;
    if local_index == u32::MAX {
        return Ok(None);
    }
    mesh.materials.get(local_index as usize).ok_or_else(|| {
        ModelResourceError::InvalidData {
            detail: format!(
                "mesh {mesh_name:?} face {face_index} references missing local material {local_index}"
            ),
        }
    })?;
    Ok(Some(local_index as usize))
}

fn copy_nodes(
    scene: &ufbx::Scene,
    mesh_indices: &HashMap<u32, usize>,
    meshes: &[StaticTriangleMesh],
    material_indices: &HashMap<u32, usize>,
    limits: &ModelDecodeLimits,
    budget: &mut OwnedBudget,
    working: &mut WorkingBudget,
) -> Result<Vec<MeshSceneNode>, ModelResourceError> {
    let mut node_indices = working.hash_map_with_capacity(scene.nodes.count)?;
    for (index, node) in scene.nodes.iter().enumerate() {
        if node_indices
            .insert(node.element.element_id, index)
            .is_some()
        {
            return Err(ModelResourceError::InvalidData {
                detail: format!("duplicate node element id {}", node.element.element_id),
            });
        }
        enforce_limit(
            "hierarchy depth",
            node.node_depth as usize,
            limits.max_hierarchy_depth,
        )?;
    }
    let mut nodes = budget.vec_with_capacity::<MeshSceneNode>(scene.nodes.count)?;
    for node in &scene.nodes {
        let name = bounded_element_name(&node.element.name);
        let parent = node
            .parent
            .as_ref()
            .map(|parent| parent.element.element_id)
            .map(|parent_id| {
                node_indices.get(&parent_id).copied().ok_or_else(|| {
                    ModelResourceError::InvalidData {
                        detail: format!("node {name:?} references missing parent {parent_id}"),
                    }
                })
            })
            .transpose()?;
        let mesh =
            node.mesh
                .as_ref()
                .map(|mesh| mesh.element.element_id)
                .map(|mesh_id| {
                    mesh_indices.get(&mesh_id).copied().ok_or_else(|| {
                        ModelResourceError::InvalidData {
                            detail: format!("node {name:?} references missing mesh {mesh_id}"),
                        }
                    })
                })
                .transpose()?;
        let source_materials = if !node.materials.is_empty() {
            Some(&node.materials)
        } else {
            node.mesh.as_ref().map(|mesh| &mesh.materials)
        };
        let mut material_slots = budget
            .vec_with_capacity::<usize>(source_materials.map_or(0, |materials| materials.count))?;
        if let Some(source_materials) = source_materials {
            for material in source_materials {
                let index = material_indices
                    .get(&material.element.element_id)
                    .copied()
                    .ok_or_else(|| ModelResourceError::InvalidData {
                        detail: format!(
                            "node {name:?} references material {} absent from the scene",
                            material.element.element_id
                        ),
                    })?;
                material_slots.push(index);
            }
        }
        if let Some(mesh_index) = mesh {
            let required_slots = meshes[mesh_index]
                .primitives
                .iter()
                .filter_map(|primitive| primitive.material_slot)
                .max()
                .map_or(0, |slot| slot.saturating_add(1));
            if material_slots.len() < required_slots {
                return Err(ModelResourceError::InvalidData {
                    detail: format!(
                        "node {name:?} supplies {} material slot(s), but its mesh requires {required_slots}",
                        material_slots.len()
                    ),
                });
            }
        }
        nodes.push(MeshSceneNode {
            name: copy_owned_text(&name, budget)?,
            parent,
            mesh,
            material_slots,
            local_transform: checked_matrix(node.node_to_parent, "local transform", &name)?,
            world_transform: checked_matrix(node.node_to_world, "world transform", &name)?,
            visible: node.visible,
        });
    }
    validate_hierarchy(&nodes, limits.max_hierarchy_depth)?;
    Ok(nodes)
}

fn validate_hierarchy(nodes: &[MeshSceneNode], max_depth: usize) -> Result<(), ModelResourceError> {
    for start in 0..nodes.len() {
        let mut cursor = Some(start);
        let mut depth = 0_usize;
        while let Some(index) = cursor {
            depth = depth.saturating_add(1);
            if depth > max_depth.saturating_add(1) {
                return Err(ModelResourceError::InvalidData {
                    detail: format!(
                        "node {:?} has a cyclic or over-depth parent chain",
                        nodes[start].name
                    ),
                });
            }
            cursor = nodes[index].parent;
        }
    }
    Ok(())
}
