use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::time::Duration;

use super::{
    ModelDecodeLimits, ModelDecoderIdentity, ModelDiagnosticCode, ModelNormalizationSettings,
    ModelResourceError, ModelResourceKey, ModelResourceService,
};
use crate::core::cache::CacheManager;
use crate::model::asset::{Asset, AssetKind};

const TRIANGLE_FBX: &str = r#"; FBX 7.4.0 project file
FBXHeaderExtension:  {
    FBXHeaderVersion: 1003
    FBXVersion: 7400
    Creator: "RuViE model-resource test"
}
GlobalSettings:  {
    Version: 1000
    Properties70:  {
        P: "UpAxis", "int", "Integer", "",1
        P: "UpAxisSign", "int", "Integer", "",1
        P: "FrontAxis", "int", "Integer", "",2
        P: "FrontAxisSign", "int", "Integer", "",-1
        P: "CoordAxis", "int", "Integer", "",0
        P: "CoordAxisSign", "int", "Integer", "",1
        P: "UnitScaleFactor", "double", "Number", "",100
    }
}
Definitions:  {
    Version: 100
    Count: 3
    ObjectType: "Geometry" { Count: 1 }
    ObjectType: "Model" { Count: 1 }
    ObjectType: "Material" { Count: 1 }
}
Objects:  {
    Geometry: 1001, "Geometry::Triangle", "Mesh" {
        GeometryVersion: 124
        Vertices: *9 { a: 0,0,0, 1,0,0, 0,1,0 }
        PolygonVertexIndex: *3 { a: 0,1,-3 }
        LayerElementUV: 0 {
            Version: 101
            Name: "UVMap"
            MappingInformationType: "ByPolygonVertex"
            ReferenceInformationType: "IndexToDirect"
            UV: *6 { a: 0,0, 1,0, 0,1 }
            UVIndex: *3 { a: 0,1,2 }
        }
        Layer: 0 {
            Version: 100
            LayerElement: { Type: "LayerElementUV", TypedIndex: 0 }
        }
    }
    Model: 1002, "Model::Triangle", "Mesh" {
        Version: 232
        Properties70:  {
            P: "Lcl Translation", "Lcl Translation", "", "A",2,3,4
            P: "Lcl Rotation", "Lcl Rotation", "", "A",0,0,0
            P: "Lcl Scaling", "Lcl Scaling", "", "A",1,1,1
        }
        Shading: T
        Culling: "CullingOff"
    }
    Material: 1003, "Material::Red", "" {
        Version: 102
        ShadingModel: "lambert"
        MultiLayer: 0
        Properties70:  {
            P: "DiffuseColor", "Color", "", "A",0.8,0.2,0.1
            P: "DiffuseFactor", "Number", "", "A",1
        }
    }
}
Connections:  {
    C: "OO",1001,1002
    C: "OO",1003,1002
    C: "OO",1002,0
}
"#;

fn service() -> (Arc<CacheManager>, ModelResourceService) {
    let cache = Arc::new(CacheManager::new());
    let service = ModelResourceService::new(Arc::clone(&cache));
    (cache, service)
}

fn instanced_material_fbx() -> String {
    TRIANGLE_FBX
        .replace("    Count: 3", "    Count: 5")
        .replace(
            "    ObjectType: \"Model\" { Count: 1 }",
            "    ObjectType: \"Model\" { Count: 2 }",
        )
        .replace(
            "    ObjectType: \"Material\" { Count: 1 }",
            "    ObjectType: \"Material\" { Count: 2 }",
        )
        .replace(
            "    Material: 1003",
            "    Model: 1004, \"Model::TriangleBlue\", \"Mesh\" {\n        Version: 232\n        Properties70:  {\n            P: \"Lcl Translation\", \"Lcl Translation\", \"\", \"A\",5,0,0\n            P: \"Lcl Rotation\", \"Lcl Rotation\", \"\", \"A\",0,0,0\n            P: \"Lcl Scaling\", \"Lcl Scaling\", \"\", \"A\",1,1,1\n        }\n        Shading: T\n        Culling: \"CullingOff\"\n    }\n    Material: 1005, \"Material::Blue\", \"\" {\n        Version: 102\n        ShadingModel: \"lambert\"\n        MultiLayer: 0\n        Properties70:  {\n            P: \"DiffuseColor\", \"Color\", \"\", \"A\",0.1,0.2,0.9\n            P: \"DiffuseFactor\", \"Number\", \"\", \"A\",1\n        }\n    }\n    Material: 1003",
        )
        .replace(
            "    C: \"OO\",1001,1002",
            "    C: \"OO\",1001,1002\n    C: \"OO\",1001,1004",
        )
        .replace(
            "    C: \"OO\",1003,1002",
            "    C: \"OO\",1003,1002\n    C: \"OO\",1005,1004",
        )
        .replace(
            "    C: \"OO\",1002,0",
            "    C: \"OO\",1002,0\n    C: \"OO\",1004,0",
        )
}

#[test]
fn decodes_owned_static_triangle_hierarchy_material_and_uv0() {
    let (_cache, service) = service();
    let scene = service
        .decode_fbx_bytes(TRIANGLE_FBX.as_bytes())
        .expect("decode triangle FBX");

    assert_eq!(scene.meshes.len(), 1);
    let mesh = &scene.meshes[0];
    assert_eq!(mesh.vertices.len(), 3);
    assert_eq!(mesh.indices, [0, 1, 2]);
    assert_eq!(
        mesh.vertices
            .iter()
            .map(|vertex| vertex.position)
            .collect::<Vec<_>>(),
        [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
    );
    assert_eq!(mesh.primitives.len(), 1);
    assert!(mesh.has_uv0);
    assert_eq!(mesh.vertices[0].uv0, [0.0, 0.0]);
    assert_eq!(mesh.vertices[1].uv0, [1.0, 0.0]);
    assert!(mesh.vertices.iter().all(|vertex| {
        vertex.position.iter().all(|value| value.is_finite())
            && vertex.normal.iter().all(|value| value.is_finite())
    }));
    let [ia, ib, ic] = [mesh.indices[0], mesh.indices[1], mesh.indices[2]];
    let [a, b, c] = [
        mesh.vertices[ia as usize].position,
        mesh.vertices[ib as usize].position,
        mesh.vertices[ic as usize].position,
    ];
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let winding_normal = [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ];
    let vertex_normal = mesh.vertices[ia as usize].normal;
    assert!(
        winding_normal
            .iter()
            .zip(vertex_normal)
            .map(|(left, right)| left * right)
            .sum::<f32>()
            > 0.0,
        "decoded winding and generated/source normals must agree"
    );
    assert_eq!(scene.materials.len(), 1);
    assert_eq!(scene.materials[0].name, "Red");
    assert!((scene.materials[0].base_color[0] - 0.8).abs() < 0.001);

    let model_node = scene
        .nodes
        .iter()
        .find(|node| node.name == "Triangle" && node.mesh == Some(0))
        .expect("mesh node");
    assert!(model_node.parent.is_some());
    assert_eq!(
        model_node.local_transform,
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, -1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [2.0, -3.0, 4.0, 1.0],
        ]
    );
    assert_eq!(model_node.material_slots, [0]);
}

#[test]
fn mesh_scene_has_no_parser_lifetime_or_thread_affinity() {
    fn require_owned_thread_safe_static<T: Send + Sync + 'static>() {}
    require_owned_thread_safe_static::<super::MeshScene>();

    let bytes = TRIANGLE_FBX.as_bytes().to_vec();
    let (_cache, service) = service();
    let scene = service
        .decode_fbx_bytes(&bytes)
        .expect("decode owned scene");
    drop(bytes);
    assert_eq!(scene.meshes[0].vertices.len(), 3);
}

#[test]
fn shared_mesh_instances_preserve_distinct_node_material_slots() {
    let (_cache, service) = service();
    let scene = service
        .decode_fbx_bytes(instanced_material_fbx().as_bytes())
        .expect("decode instanced material fixture");
    assert_eq!(scene.meshes.len(), 1, "geometry must stay shared");
    assert_eq!(scene.meshes[0].primitives[0].material_slot, Some(0));

    let red = scene
        .nodes
        .iter()
        .find(|node| node.name == "Triangle")
        .expect("red instance");
    let blue = scene
        .nodes
        .iter()
        .find(|node| node.name == "TriangleBlue")
        .expect("blue instance");
    assert_eq!(scene.materials[red.material_slots[0]].name, "Red");
    assert_eq!(scene.materials[blue.material_slots[0]].name, "Blue");
    assert_ne!(red.material_slots, blue.material_slots);
}

#[test]
fn coordinate_normalization_scales_centimeters_to_meters_exactly() {
    let centimeters = TRIANGLE_FBX.replace(
        "P: \"UnitScaleFactor\", \"double\", \"Number\", \"\",100",
        "P: \"UnitScaleFactor\", \"double\", \"Number\", \"\",1",
    );
    let (_cache, service) = service();
    let scene = service
        .decode_fbx_bytes(centimeters.as_bytes())
        .expect("centimeter scene");
    let node = scene
        .nodes
        .iter()
        .find(|node| node.name == "Triangle" && node.mesh == Some(0))
        .expect("mesh node");
    assert_eq!(node.local_transform[3], [0.02, -0.03, 0.04, 1.0]);
    assert_eq!(scene.source_metadata.original_unit_meters, 0.01);
}

#[test]
fn malformed_fbx_fails_without_publishing_a_cache_entry() {
    let (cache, service) = service();
    let error = service
        .decode_fbx_bytes(b"definitely not an FBX file")
        .expect_err("malformed input must fail");
    assert!(matches!(error, ModelResourceError::Decode { .. }));
    assert_eq!(cache.model_scene_cache_len(), 0);
}

#[test]
fn source_node_and_owned_scene_budgets_fail_closed() {
    let cache = Arc::new(CacheManager::new());
    let limits = ModelDecodeLimits {
        max_source_bytes: 16,
        ..ModelDecodeLimits::default()
    };
    let source_limited =
        ModelResourceService::with_limits(Arc::clone(&cache), limits).expect("valid source limit");
    assert!(matches!(
        source_limited.decode_fbx_bytes(TRIANGLE_FBX.as_bytes()),
        Err(ModelResourceError::BudgetExceeded {
            resource: "source bytes",
            ..
        })
    ));

    let limits = ModelDecodeLimits {
        max_nodes: 1,
        ..ModelDecodeLimits::default()
    };
    let node_limited =
        ModelResourceService::with_limits(Arc::clone(&cache), limits).expect("valid node limit");
    assert!(matches!(
        node_limited.decode_fbx_bytes(TRIANGLE_FBX.as_bytes()),
        Err(ModelResourceError::BudgetExceeded {
            resource: "nodes",
            ..
        })
    ));

    let limits = ModelDecodeLimits {
        max_scene_bytes: 64,
        ..ModelDecodeLimits::default()
    };
    let scene_limited =
        ModelResourceService::with_limits(cache, limits).expect("valid scene limit");
    assert!(matches!(
        scene_limited.decode_fbx_bytes(TRIANGLE_FBX.as_bytes()),
        Err(ModelResourceError::BudgetExceeded {
            resource: "owned scene bytes",
            ..
        })
    ));

    let limits = ModelDecodeLimits {
        max_working_bytes: 1,
        ..ModelDecodeLimits::default()
    };
    let working_limited = ModelResourceService::with_limits(Arc::new(CacheManager::new()), limits)
        .expect("valid working limit");
    assert!(matches!(
        working_limited.decode_fbx_bytes(TRIANGLE_FBX.as_bytes()),
        Err(ModelResourceError::BudgetExceeded {
            resource: "decode working bytes",
            ..
        })
    ));
}

#[test]
fn disabled_missing_normal_generation_keeps_zero_normals() {
    let normalization = ModelNormalizationSettings {
        generate_missing_normals: false,
        ..ModelNormalizationSettings::default()
    };
    let service = ModelResourceService::with_configuration(
        Arc::new(CacheManager::new()),
        ModelDecodeLimits::default(),
        normalization,
    )
    .expect("configuration");
    let scene = service
        .decode_fbx_bytes(TRIANGLE_FBX.as_bytes())
        .expect("decode without generated normals");
    assert!(
        scene.meshes[0]
            .vertices
            .iter()
            .all(|vertex| vertex.normal == [0.0; 3])
    );
}

#[test]
fn concurrent_identical_decodes_share_one_flight_and_scene() {
    const WORKERS: usize = 12;
    let cache = Arc::new(CacheManager::new());
    let service = ModelResourceService::new(Arc::clone(&cache));
    let source = Arc::new(TRIANGLE_FBX.as_bytes().to_vec());
    let barrier = Arc::new(Barrier::new(WORKERS));
    let mut workers = Vec::new();
    for _ in 0..WORKERS {
        let service = service.clone();
        let source = Arc::clone(&source);
        let barrier = Arc::clone(&barrier);
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            service
                .decode_fbx_bytes(&source)
                .expect("concurrent decode")
        }));
    }
    let scenes = workers
        .into_iter()
        .map(|worker| worker.join().expect("worker"))
        .collect::<Vec<_>>();
    assert!(
        scenes
            .iter()
            .skip(1)
            .all(|scene| Arc::ptr_eq(&scenes[0], scene))
    );
    assert_eq!(cache.model_scene_decode_count(), 1);
}

#[test]
fn concurrent_failed_flight_wakes_every_waiter_with_one_attempt() {
    const WORKERS: usize = 8;
    let cache = Arc::new(CacheManager::new());
    let key = ModelResourceKey {
        source_sha256: [7; 32],
        decoder: ModelDecoderIdentity {
            implementation: "failure-test",
            version: "1",
        },
        normalization: ModelNormalizationSettings::default(),
        supported_feature_version: 1,
    };
    let limits = ModelDecodeLimits::default();
    let barrier = Arc::new(Barrier::new(WORKERS));
    let attempts = Arc::new(AtomicUsize::new(0));
    let mut workers = Vec::new();
    for _ in 0..WORKERS {
        let cache = Arc::clone(&cache);
        let key = key.clone();
        let limits = limits.clone();
        let barrier = Arc::clone(&barrier);
        let attempts = Arc::clone(&attempts);
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            cache.get_or_decode_model_scene(key, limits, || {
                attempts.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(50));
                Err(ModelResourceError::Decode {
                    detail: "expected concurrent failure".to_string(),
                })
            })
        }));
    }
    for worker in workers {
        let error = worker
            .join()
            .expect("worker")
            .expect_err("flight must fail");
        assert!(matches!(error, ModelResourceError::Decode { .. }));
    }
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}

#[test]
fn cache_identity_reuses_the_same_owned_scene() {
    let (cache, service) = service();
    let first = service
        .decode_fbx_bytes(TRIANGLE_FBX.as_bytes())
        .expect("first decode");
    let second = service
        .decode_fbx_bytes(TRIANGLE_FBX.as_bytes())
        .expect("cached decode");
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(cache.model_scene_cache_len(), 1);
    assert!(cache.model_scene_cache_resident_bytes() > 0);
}

#[test]
fn cached_scene_is_revalidated_for_a_stricter_service_budget() {
    let (cache, service) = service();
    service
        .decode_fbx_bytes(TRIANGLE_FBX.as_bytes())
        .expect("populate shared cache");
    let limits = ModelDecodeLimits {
        max_nodes: 1,
        ..ModelDecodeLimits::default()
    };
    let strict = ModelResourceService::with_limits(cache, limits).expect("strict service");
    assert!(matches!(
        strict.decode_fbx_bytes(TRIANGLE_FBX.as_bytes()),
        Err(ModelResourceError::BudgetExceeded {
            resource: "nodes",
            ..
        })
    ));
}

#[test]
fn shared_model_cache_is_count_bounded_and_rebuildable() {
    let (cache, service) = service();
    let first_bytes = format!("{TRIANGLE_FBX}\n; cache identity 0");
    let first = service
        .decode_fbx_bytes(first_bytes.as_bytes())
        .expect("first scene");
    for identity in 1..33 {
        let bytes = format!("{TRIANGLE_FBX}\n; cache identity {identity}");
        service
            .decode_fbx_bytes(bytes.as_bytes())
            .expect("bounded cache scene");
    }
    assert_eq!(cache.model_scene_cache_len(), 32);
    let rebuilt = service
        .decode_fbx_bytes(first_bytes.as_bytes())
        .expect("rebuild evicted scene");
    assert!(!Arc::ptr_eq(&first, &rebuilt));
    assert_eq!(cache.model_scene_cache_len(), 32);
}

#[test]
fn file_and_asset_decode_verify_extension_and_import_fingerprint() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("Triangle.FBX");
    std::fs::write(&path, TRIANGLE_FBX).expect("write fixture");
    let (_cache, service) = service();
    assert_eq!(
        service
            .load_fbx_file(&path)
            .expect("file decode")
            .meshes
            .len(),
        1
    );

    let mut asset = Asset::new("Triangle", &path.to_string_lossy(), AssetKind::Model3D);
    asset.verify_imported_content(TRIANGLE_FBX.as_bytes());
    assert_eq!(
        service
            .load_asset(&asset)
            .expect("asset decode")
            .meshes
            .len(),
        1
    );

    std::fs::write(&path, format!("{TRIANGLE_FBX}\n; changed")).expect("replace fixture");
    assert!(matches!(
        service.load_asset(&asset),
        Err(ModelResourceError::FingerprintMismatch { .. })
    ));
}

#[test]
fn unsupported_scene_features_are_explicit_diagnostics() {
    let camera = TRIANGLE_FBX.replace(
        "Count: 3",
        "Count: 4\n    ObjectType: \"NodeAttribute\" { Count: 1 }",
    );
    let camera = camera.replace(
        "    Material: 1003",
        "    NodeAttribute: 1004, \"NodeAttribute::Camera\", \"Camera\" {\n        TypeFlags: \"Camera\"\n        GeometryVersion: 124\n    }\n    Model: 1005, \"Model::Camera\", \"Camera\" {\n        Version: 232\n    }\n    Material: 1003",
    );
    let camera = camera.replace(
        "    C: \"OO\",1002,0",
        "    C: \"OO\",1004,1005\n    C: \"OO\",1005,0\n    C: \"OO\",1002,0",
    );
    let (_cache, service) = service();
    let scene = service
        .decode_fbx_bytes(camera.as_bytes())
        .expect("camera fixture still has a supported triangle");
    assert!(
        scene
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == ModelDiagnosticCode::CameraUnsupported)
    );
}

#[test]
fn skin_deformer_is_reported_while_static_geometry_remains_available() {
    let skinned = TRIANGLE_FBX.replace(
        "Count: 3",
        "Count: 6\n    ObjectType: \"Deformer\" { Count: 2 }",
    );
    let skinned = skinned.replace(
        "    ObjectType: \"Model\" { Count: 1 }",
        "    ObjectType: \"Model\" { Count: 2 }",
    );
    let skinned = skinned.replace(
        "    Material: 1003",
        "    Deformer: 3001, \"Deformer::Skin\", \"Skin\" {\n        Version: 101\n        Link_DeformAcuracy: 50\n    }\n    Deformer: 3002, \"SubDeformer::Cluster\", \"Cluster\" {\n        Version: 100\n        UserData: \"\", \"\"\n        Indexes: *3 { a: 0,1,2 }\n        Weights: *3 { a: 1,1,1 }\n        Transform: *16 { a: 1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1 }\n        TransformLink: *16 { a: 1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1 }\n    }\n    Model: 3003, \"Model::Bone\", \"LimbNode\" {\n        Version: 232\n        TypeFlags: \"Skeleton\"\n    }\n    Material: 1003",
    );
    let skinned = skinned.replace(
        "    C: \"OO\",1001,1002",
        "    C: \"OO\",3001,1001\n    C: \"OO\",3002,3001\n    C: \"OO\",3003,3002\n    C: \"OO\",3003,0\n    C: \"OO\",1001,1002",
    );
    let (_cache, service) = service();
    let scene = service
        .decode_fbx_bytes(skinned.as_bytes())
        .expect("skin must not hide supported static geometry");
    assert!(
        scene
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == ModelDiagnosticCode::SkinningUnsupported)
    );
    assert_eq!(scene.meshes[0].indices.len(), 3);
}

#[test]
fn morph_deformer_is_reported_while_static_geometry_remains_available() {
    let morphed = TRIANGLE_FBX.replace(
        "Count: 3",
        "Count: 6\n    ObjectType: \"Deformer\" { Count: 2 }",
    );
    let morphed = morphed.replace(
        "    ObjectType: \"Geometry\" { Count: 1 }",
        "    ObjectType: \"Geometry\" { Count: 2 }",
    );
    let morphed = morphed.replace(
        "    Material: 1003",
        "    Deformer: 4001, \"Deformer::Blend\", \"BlendShape\" {\n        Version: 100\n    }\n    Deformer: 4002, \"SubDeformer::Smile\", \"BlendShapeChannel\" {\n        Version: 100\n        DeformPercent: 0\n        FullWeights: *1 { a: 100 }\n    }\n    Geometry: 4003, \"Geometry::Smile\", \"Shape\" {\n        Version: 100\n        Indexes: *1 { a: 0 }\n        Vertices: *3 { a: 0,0,0.1 }\n        Normals: *3 { a: 0,0,0 }\n    }\n    Material: 1003",
    );
    let morphed = morphed.replace(
        "    C: \"OO\",1001,1002",
        "    C: \"OO\",4001,1001\n    C: \"OO\",4002,4001\n    C: \"OO\",4003,4002\n    C: \"OO\",1001,1002",
    );
    let (_cache, service) = service();
    let scene = service
        .decode_fbx_bytes(morphed.as_bytes())
        .expect("morph must not hide supported static geometry");
    assert!(
        scene
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == ModelDiagnosticCode::MorphTargetsUnsupported)
    );
    assert_eq!(scene.meshes[0].indices.len(), 3);
}

#[test]
fn external_texture_reference_is_not_loaded_and_is_diagnosed() {
    let textured = TRIANGLE_FBX.replace(
        "Count: 3",
        "Count: 5\n    ObjectType: \"Texture\" { Count: 1 }\n    ObjectType: \"Video\" { Count: 1 }",
    );
    let textured = textured.replace(
        "    Material: 1003",
        "    Video: 2001, \"Video::Outside\", \"Clip\" {\n        Type: \"Clip\"\n        FileName: \"outside.png\"\n        RelativeFilename: \"outside.png\"\n    }\n    Texture: 2002, \"Texture::Outside\", \"TextureVideoClip\" {\n        Type: \"TextureVideoClip\"\n        Version: 202\n        TextureName: \"Texture::Outside\"\n        Media: \"Video::Outside\"\n        FileName: \"outside.png\"\n        RelativeFilename: \"outside.png\"\n    }\n    Material: 1003",
    );
    let textured = textured.replace(
        "    C: \"OO\",1003,1002",
        "    C: \"OO\",2001,2002\n    C: \"OP\",2002,1003,\"DiffuseColor\"\n    C: \"OO\",1003,1002",
    );
    let textured = textured.replace(
        "        TextureName: \"Texture::Outside\"",
        "        TextureName: \"Texture::Outside\"\n        Properties70:  {\n            P: \"UVSet\", \"KString\", \"\", \"\",\"SecondaryUV\"\n            P: \"WrapModeU\", \"enum\", \"\", \"\",1\n            P: \"WrapModeV\", \"enum\", \"\", \"\",1\n            P: \"Translation\", \"Vector3D\", \"Vector\", \"\",0.25,0,0\n        }",
    );
    let (_cache, service) = service();
    let scene = service
        .decode_fbx_bytes(textured.as_bytes())
        .expect("external texture must not prevent static geometry decode");
    assert!(scene.textures.is_empty());
    assert!(
        scene
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == ModelDiagnosticCode::ExternalTextureNotLoaded })
    );
    for code in [
        ModelDiagnosticCode::TextureUvTransformUnsupported,
        ModelDiagnosticCode::TextureUvSetSelectionUnsupported,
        ModelDiagnosticCode::TextureWrapModeUnsupported,
    ] {
        assert!(
            scene
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == code),
            "missing diagnostic {code:?}"
        );
    }
}
