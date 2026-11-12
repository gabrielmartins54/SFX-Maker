//! GPU (OpenGL) canvas renderer.
//!
//! Glyph outlines are tessellated into triangles on the CPU (lyon) only when the scene or the
//! zoom bucket changes; the triangles are uploaded once and drawn by an OpenGL pass inside
//! egui's own GL context (an `egui_glow` paint callback). Pan and zoom are then just two
//! uniforms, so moving around costs no CPU work. PNG/SVG export still use the CPU rasterizer
//! (`render::raster`) so exports are independent of the GPU.

use std::sync::{Arc, Mutex};

use eframe::egui_glow::{self, ShaderVersion};
use eframe::glow::{self, HasContext};
use kurbo::{BezPath, PathEl};
use lyon_tessellation::math::point;
use lyon_tessellation::path::Path as LPath;
use lyon_tessellation::{BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex, VertexBuffers};

use crate::model::scene::Scene;

/// Triangle list in scene space.
#[derive(Default, Debug, Clone)]
pub struct MeshData {
    /// x, y pairs.
    pub vertices: Vec<f32>,
    pub indices: Vec<u32>,
}

fn to_lyon(path: &BezPath) -> LPath {
    let p = |p: kurbo::Point| point(p.x as f32, p.y as f32);
    let mut b = LPath::builder();
    let mut open = false;
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(a) => {
                if open {
                    b.end(true);
                }
                b.begin(p(a));
                open = true;
            }
            PathEl::LineTo(a) if open => {
                b.line_to(p(a));
            }
            PathEl::QuadTo(c, a) if open => {
                b.quadratic_bezier_to(p(c), p(a));
            }
            PathEl::CurveTo(c1, c2, a) if open => {
                b.cubic_bezier_to(p(c1), p(c2), p(a));
            }
            PathEl::ClosePath if open => {
                b.end(true);
                open = false;
            }
            _ => {}
        }
    }
    if open {
        b.end(true);
    }
    b.build()
}

/// Tessellates every letter (non-zero fill, like the CPU renderer). `tolerance` is the maximum
/// curve flattening error in scene units. Letters that fail to tessellate are skipped.
pub fn tessellate_scene(scene: &Scene, tolerance: f32) -> MeshData {
    let mut tess = FillTessellator::new();
    let mut buffers: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
    let opts = FillOptions::tolerance(tolerance.max(1e-4)).with_fill_rule(FillRule::NonZero);
    for letter in &scene.letters {
        if letter.glyph_path.elements().is_empty() {
            continue;
        }
        let path = to_lyon(&letter.placed_path());
        let before = (buffers.vertices.len(), buffers.indices.len());
        let res = tess.tessellate_path(
            &path,
            &opts,
            &mut BuffersBuilder::new(&mut buffers, |v: FillVertex| v.position().to_array()),
        );
        if res.is_err() {
            // Roll back partial output of the failed letter.
            buffers.vertices.truncate(before.0);
            buffers.indices.truncate(before.1);
        }
    }
    MeshData {
        vertices: buffers.vertices.iter().flat_map(|v| *v).collect(),
        indices: buffers.indices,
    }
}

struct Resources {
    program: glow::Program,
    vao: Option<glow::VertexArray>,
    vbo: glow::Buffer,
    ibo: glow::Buffer,
    u_scale: Option<glow::UniformLocation>,
    u_offset: Option<glow::UniformLocation>,
    a_pos: u32,
}

#[derive(Default)]
struct Inner {
    pending: Option<MeshData>,
    res: Option<Resources>,
    index_count: i32,
    error: Option<String>,
}

/// Handle shared between the UI thread (uploads, uniforms) and the GL callback.
#[derive(Clone, Default)]
pub struct GpuScene {
    inner: Arc<Mutex<Inner>>,
}

const VERT: &str = "
#if NEW_SHADER_INTERFACE
#define ATTRIBUTE in
#else
#define ATTRIBUTE attribute
#endif
ATTRIBUTE vec2 a_pos;
uniform vec2 u_scale;
uniform vec2 u_offset;
void main() {
    gl_Position = vec4(a_pos * u_scale + u_offset, 0.0, 1.0);
}
";

const FRAG: &str = "
#ifdef GL_ES
precision mediump float;
#endif
#if NEW_SHADER_INTERFACE
out vec4 f_color;
#endif
void main() {
#if NEW_SHADER_INTERFACE
    f_color = vec4(0.0, 0.0, 0.0, 1.0);
#else
    gl_FragColor = vec4(0.0, 0.0, 0.0, 1.0);
#endif
}
";

fn compile(gl: &glow::Context, sv: ShaderVersion, kind: u32, body: &str) -> Result<glow::Shader, String> {
    let src = format!(
        "{}\n#define NEW_SHADER_INTERFACE {}\n{}",
        sv.version_declaration().trim_end(),
        i32::from(sv.is_new_shader_interface()),
        body
    );
    // SAFETY: plain GL calls on the context egui gave us, on the render thread.
    unsafe {
        let s = gl.create_shader(kind)?;
        gl.shader_source(s, &src);
        gl.compile_shader(s);
        if gl.get_shader_compile_status(s) {
            Ok(s)
        } else {
            let log = gl.get_shader_info_log(s);
            gl.delete_shader(s);
            Err(format!("Shader compile failed: {log}"))
        }
    }
}

fn create_resources(gl: &glow::Context) -> Result<Resources, String> {
    let sv = ShaderVersion::get(gl);
    // SAFETY: plain GL calls on the context egui gave us, on the render thread.
    unsafe {
        let vs = compile(gl, sv, glow::VERTEX_SHADER, VERT)?;
        let fs = compile(gl, sv, glow::FRAGMENT_SHADER, FRAG)?;
        let program = gl.create_program()?;
        gl.attach_shader(program, vs);
        gl.attach_shader(program, fs);
        gl.link_program(program);
        let linked = gl.get_program_link_status(program);
        gl.detach_shader(program, vs);
        gl.detach_shader(program, fs);
        gl.delete_shader(vs);
        gl.delete_shader(fs);
        if !linked {
            let log = gl.get_program_info_log(program);
            gl.delete_program(program);
            return Err(format!("Shader link failed: {log}"));
        }
        let a_pos = gl.get_attrib_location(program, "a_pos").ok_or("missing attribute a_pos")?;
        let vao = if sv.is_new_shader_interface() { Some(gl.create_vertex_array()?) } else { None };
        Ok(Resources {
            u_scale: gl.get_uniform_location(program, "u_scale"),
            u_offset: gl.get_uniform_location(program, "u_offset"),
            vbo: gl.create_buffer()?,
            ibo: gl.create_buffer()?,
            program,
            vao,
            a_pos,
        })
    }
}

fn upload(gl: &glow::Context, r: &Resources, mesh: &MeshData) {
    let vbytes: Vec<u8> = mesh.vertices.iter().flat_map(|f| f.to_ne_bytes()).collect();
    let ibytes: Vec<u8> = mesh.indices.iter().flat_map(|i| i.to_ne_bytes()).collect();
    // SAFETY: plain GL calls on the context egui gave us, on the render thread.
    unsafe {
        gl.bind_vertex_array(r.vao);
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(r.vbo));
        gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, &vbytes, glow::STATIC_DRAW);
        gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(r.ibo));
        gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, &ibytes, glow::STATIC_DRAW);
        gl.bind_vertex_array(None);
    }
}

fn draw(gl: &glow::Context, r: &Resources, count: i32, scale: [f32; 2], offset: [f32; 2]) {
    // SAFETY: plain GL calls on the context egui gave us, on the render thread.
    unsafe {
        gl.disable(glow::CULL_FACE);
        gl.use_program(Some(r.program));
        gl.uniform_2_f32(r.u_scale.as_ref(), scale[0], scale[1]);
        gl.uniform_2_f32(r.u_offset.as_ref(), offset[0], offset[1]);
        gl.bind_vertex_array(r.vao);
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(r.vbo));
        gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(r.ibo));
        gl.enable_vertex_attrib_array(r.a_pos);
        gl.vertex_attrib_pointer_f32(r.a_pos, 2, glow::FLOAT, false, 8, 0);
        gl.draw_elements(glow::TRIANGLES, count, glow::UNSIGNED_INT, 0);
        gl.disable_vertex_attrib_array(r.a_pos);
        gl.bind_vertex_array(None);
        gl.bind_buffer(glow::ARRAY_BUFFER, None);
        gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, None);
        gl.use_program(None);
    }
}

impl GpuScene {
    /// Queues a new mesh; it is uploaded at the next paint.
    pub fn set_mesh(&self, mesh: MeshData) {
        if let Ok(mut g) = self.inner.lock() {
            g.pending = Some(mesh);
        }
    }

    /// Last GL setup error, if any (e.g. shader failed to compile).
    pub fn take_error(&self) -> Option<String> {
        self.inner.lock().ok().and_then(|mut g| g.error.take())
    }

    /// Paint callback that draws the scene into `rect` (egui points). `pan` / `zoom` map scene
    /// space to rect-local points: `local = pan + scene * zoom`.
    pub fn callback(&self, rect: egui::Rect, pan: egui::Vec2, zoom: f64) -> egui::PaintCallback {
        let (w, h) = (rect.width().max(1.0), rect.height().max(1.0));
        let z = zoom as f32;
        let scale = [2.0 * z / w, -2.0 * z / h];
        let offset = [2.0 * pan.x / w - 1.0, 1.0 - 2.0 * pan.y / h];
        let shared = Arc::clone(&self.inner);
        let cb = egui_glow::CallbackFn::new(move |_info, painter| {
            let gl = painter.gl();
            let Ok(mut g) = shared.lock() else { return };
            if g.res.is_none() && g.error.is_none() {
                match create_resources(gl) {
                    Ok(r) => g.res = Some(r),
                    Err(e) => g.error = Some(e),
                }
            }
            if let Some(mesh) = g.pending.take() {
                g.index_count = i32::try_from(mesh.indices.len()).unwrap_or(0);
                if let Some(r) = &g.res {
                    upload(gl, r, &mesh);
                }
            }
            if let Some(r) = &g.res
                && g.index_count > 0
            {
                draw(gl, r, g.index_count, scale, offset);
            }
        });
        egui::PaintCallback { rect, callback: Arc::new(cb) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::text_to_paths::{text_to_paths, FontData};
    use crate::model::scene::LayoutDirection;
    use kurbo::Shape;

    fn tri_area(m: &MeshData) -> f64 {
        m.indices
            .chunks(3)
            .map(|t| {
                let p = |i: u32| (f64::from(m.vertices[2 * i as usize]), f64::from(m.vertices[2 * i as usize + 1]));
                let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
                ((b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1)).abs() / 2.0
            })
            .sum()
    }

    #[test]
    fn square_is_two_triangles() {
        let mut sq = BezPath::new();
        sq.move_to((0.0, 0.0));
        sq.line_to((10.0, 0.0));
        sq.line_to((10.0, 10.0));
        sq.line_to((0.0, 10.0));
        sq.close_path();
        let scene = Scene {
            letters: vec![crate::model::letter::Letter::new('x', sq, kurbo::Affine::translate((5.0, 5.0)))],
            ..Scene::default()
        };
        let m = tessellate_scene(&scene, 0.1);
        assert_eq!(m.indices.len(), 6);
        assert!((tri_area(&m) - 100.0).abs() < 1e-3);
    }

    #[test]
    fn glyph_area_matches_path_area_including_holes() {
        let font = FontData::new("B", include_bytes!("../../assets/fonts/Bangers-Regular.ttf").to_vec(), 0).unwrap();
        let letters = text_to_paths(&font, "OB", LayoutDirection::Horizontal).unwrap();
        let expected: f64 = letters.iter().map(|l| l.placed_path().area().abs()).sum();
        let scene = Scene { letters, ..Scene::default() };
        let m = tessellate_scene(&scene, 0.02);
        assert!(!m.indices.is_empty() && m.indices.len().is_multiple_of(3));
        let got = tri_area(&m);
        assert!((got - expected).abs() / expected < 0.01, "got {got}, expected {expected}");
    }

    #[test]
    fn empty_scene_gives_empty_mesh() {
        assert!(tessellate_scene(&Scene::default(), 0.1).indices.is_empty());
    }
}

