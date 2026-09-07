//! Verify the production GL isolation boundary, including indirect line draws.

use super::point_invalidation_gpu::color_program;
use super::*;
use crate::model::frame::point::{
    PointConnectionParameters, PointGridParameters, PointRenderStyle,
};
use crate::rendering::gl_resources::SavedGlState;
use crate::rendering::scene_runtime::SceneTextureFormat;
use glow::HasContext;

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn point_lines_restore_foreign_indirect_and_storage_bindings_after_draw_and_failure() {
    let mut renderer = SkiaRenderer::new(256, 144, Color::black(), true, None, None).unwrap();
    let context = renderer.gpu_context.as_mut().expect("real GPU context");
    context.ensure_current().unwrap();
    context.direct_context.flush_and_submit();
    let gl = context.create_glow_context();
    let original = SavedGlState::capture(&gl);
    let query_buffers = (gl.version().major, gl.version().minor) >= (4, 4)
        || gl
            .supported_extensions()
            .contains("GL_ARB_query_buffer_object");
    let original_query_buffer = if query_buffers {
        // SAFETY: querying a supported target on this current test-owned context.
        unsafe { gl.get_parameter_buffer(glow::QUERY_BUFFER_BINDING) }
    } else {
        None
    };
    // SAFETY: these foreign sentinel buffers belong to the same current GL
    // context as the production renderer and remain live until restoration.
    let sentinels: [glow::Buffer; 9] = unsafe {
        std::array::from_fn(|_| {
            let buffer = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::SHADER_STORAGE_BUFFER, Some(buffer));
            gl.buffer_data_size(glow::SHADER_STORAGE_BUFFER, 64, glow::STATIC_DRAW);
            buffer
        })
    };
    // SAFETY: valid storage allocations back every indexed and generic binding.
    unsafe {
        for (index, buffer) in sentinels[..6].iter().enumerate() {
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, index as u32, Some(*buffer));
        }
        gl.bind_buffer(glow::DRAW_INDIRECT_BUFFER, Some(sentinels[6]));
        gl.bind_buffer(glow::SHADER_STORAGE_BUFFER, Some(sentinels[7]));
        if query_buffers {
            gl.bind_buffer(glow::QUERY_BUFFER, Some(sentinels[8]));
        }
    }
    // SAFETY: a separately owned elapsed query remains active throughout the
    // production draws. Timestamp profiling must neither end nor replace it.
    let elapsed_query = unsafe {
        let query = gl.create_query().unwrap();
        gl.begin_query(glow::TIME_ELAPSED, query);
        query
    };

    let mut scene = particle_scene(0);
    scene.source = PointSceneSource::Grid(PointGridParameters {
        counts: [3, 2, 1],
        spacing: particle_vec3(24.0, 24.0, 24.0),
        center: particle_vec3(0.0, 0.0, 0.0),
        size: 8.0.into(),
        seed: 123,
    });
    scene.point_program = Some(color_program(Color::white()));
    scene.render_style = PointRenderStyle::Lines {
        connections: PointConnectionParameters {
            min_distance: 0.0.into(),
            max_distance: 40.0.into(),
            max_neighbors: 3,
        },
        width: 2.0.into(),
        fade: 0.0.into(),
    };

    for mode in ["draw", "dense", "field_failure", "recovery"] {
        let PointSceneSource::Grid(grid) = &mut scene.source else {
            panic!("Grid")
        };
        grid.counts = if mode == "dense" {
            [65, 65, 1]
        } else {
            [3, 2, 1]
        };
        grid.spacing = if mode == "dense" {
            particle_vec3(0.0, 0.0, 0.0)
        } else {
            particle_vec3(24.0, 24.0, 24.0)
        };
        let runtime = renderer
            .scene_runtime
            .as_mut()
            .expect("production SceneRuntime");
        runtime.set_point_profiling_for_test(true).unwrap();
        if mode == "field_failure" {
            runtime.fail_next_field_evaluation_for_test();
        }
        let result = runtime.render_point(
            PointRasterRequest {
                scene: &scene,
                transform: &Affine2D::IDENTITY,
                sprites: &[],
            },
            256,
            144,
            SceneTextureFormat::Srgba8,
            [1.0; 4],
        );
        match mode {
            "dense" => assert!(result.unwrap_err().to_string().contains("candidate tests")),
            "field_failure" => assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("injected Point field")
            ),
            _ => {
                result.unwrap();
            }
        }
        // SAFETY: the owning context is still current after both success and
        // fail-closed paths. Queries do not mutate the restored foreign state.
        unsafe {
            if query_buffers {
                assert_eq!(
                    gl.get_parameter_buffer(glow::QUERY_BUFFER_BINDING),
                    Some(sentinels[8]),
                    "{mode}: profiler must restore the foreign query buffer"
                );
            }
            assert_eq!(
                gl.get_parameter_buffer(glow::DRAW_INDIRECT_BUFFER_BINDING),
                Some(sentinels[6]),
                "{mode}"
            );
            assert_eq!(
                gl.get_parameter_buffer(glow::SHADER_STORAGE_BUFFER_BINDING),
                Some(sentinels[7]),
                "{mode}"
            );
            for (index, buffer) in sentinels[..6].iter().enumerate() {
                assert_eq!(
                    gl.get_parameter_indexed_i32(glow::SHADER_STORAGE_BUFFER_BINDING, index as u32)
                        as u32,
                    buffer.0.get(),
                    "{mode}: slot {index}"
                );
            }
            assert_eq!(gl.get_error(), glow::NO_ERROR, "{mode}");
        }
    }
    original.restore(&gl);
    // SAFETY: the sentinels have been unbound by restoring the original state;
    // no production resource ever takes ownership of these test buffers.
    unsafe {
        gl.end_query(glow::TIME_ELAPSED);
        if query_buffers {
            gl.bind_buffer(glow::QUERY_BUFFER, None);
        }
        assert!(gl.get_query_parameter_u32(elapsed_query, glow::QUERY_RESULT) > 0);
        if query_buffers {
            gl.bind_buffer(glow::QUERY_BUFFER, original_query_buffer);
        }
        gl.delete_query(elapsed_query);
        assert_eq!(gl.get_error(), glow::NO_ERROR);
        for buffer in sentinels {
            gl.delete_buffer(buffer);
        }
    }
    renderer
        .gpu_context
        .as_mut()
        .unwrap()
        .direct_context
        .reset(None);
}
