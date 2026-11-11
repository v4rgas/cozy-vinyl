//! Interactive turntable inside the furnished room. SI units, +z up.
use crate::{environment::Environment, mesh::Meshes, room::Room};
use eframe::{
    egui,
    glow::{self, HasContext},
};
use glam::{Mat4, Vec2, Vec3};
use std::{
    collections::HashMap,
    f32::consts::{PI, TAU},
    num::NonZeroU32,
};

const ARM_LE: f32 = 0.2286;
const FOV: f32 = 50.0 * PI / 180.0;
const SHADOW: i32 = 2048;
const BLACK: [f32; 3] = [0.012, 0.012, 0.013];
const CHROME: [f32; 3] = [0.62, 0.63, 0.66];

fn pivot() -> Vec3 {
    let d = (ARM_LE * ARM_LE - 0.066 * 0.1209).sqrt();
    Vec3::new(
        d * (27.0_f32.to_radians()).cos(),
        d * (27.0_f32.to_radians()).sin(),
        0.030,
    )
}
fn beta() -> f32 {
    ((0.066 + 0.1209) / (2.0 * ARM_LE)).asin()
}
pub fn alpha_for_radius(radius: f32) -> f32 {
    let d = pivot().truncate().length();
    27.0_f32.to_radians()
        + PI
        + ((d * d + ARM_LE * ARM_LE - radius * radius) / (2.0 * d * ARM_LE))
            .clamp(-1.0, 1.0)
            .acos()
}
pub fn stylus_xy(angle: f32) -> [f32; 2] {
    (pivot().truncate() + Vec2::new(angle.cos(), angle.sin()) * ARM_LE).to_array()
}

#[derive(Clone, Debug)]
pub struct SceneState {
    pub platter_angle: f32,
    pub arm_angle: f32,
    pub stylus_height: f32,
    pub zoom: f32,
    pub inspect_stylus: bool,
    pub azimuth: f32,
    pub elevation: f32,
    pub motor: bool,
    pub rpm: f32,
    pub record: bool,
}
impl Default for SceneState {
    fn default() -> Self {
        Self {
            platter_angle: 0.0,
            arm_angle: alpha_for_radius(0.172),
            stylus_height: 0.005,
            zoom: 0.0,
            inspect_stylus: false,
            azimuth: -0.35,
            elevation: 0.31,
            motor: false,
            rpm: 100.0 / 3.0,
            record: false,
        }
    }
}

/// Display data derived once from a pressing; audio and playback remain independent.
#[derive(Clone, Default)]
pub struct RecordVisual {
    pub r_table: Vec<f32>,
    pub theta_end: f64,
    pub modulation: Vec<u8>,
    pub cols: usize,
    pub rows: usize,
    pub warp: [[f32; 3]; 3],
    pub scratches: Vec<[f32; 4]>,
    pub dust: f32,
    pub ecc: [f32; 2],
    pub titles: Vec<String>,
}
struct RecordUniforms {
    rows: f32,
    rev_end: f32,
    warp: [f32; 9],
    scratches: [f32; 48],
    n_scratches: i32,
    dust: f32,
    ecc: [f32; 2],
}
struct Camera {
    vp: Mat4,
}
struct Program {
    handle: glow::Program,
    uniforms: HashMap<String, glow::UniformLocation>,
}
impl Program {
    unsafe fn new(gl: &glow::Context, vertex: &str, fragment: &str) -> Result<Self, String> {
        unsafe {
            let program = gl.create_program()?;
            for (kind, source) in [
                (glow::VERTEX_SHADER, vertex),
                (glow::FRAGMENT_SHADER, fragment),
            ] {
                let shader = gl.create_shader(kind)?;
                let source = if gl.version().is_embedded {
                    source.to_owned()
                } else {
                    source
                        .replace("#version 300 es", "#version 330 core")
                        .replace("precision highp float;", "")
                        .replace("precision highp sampler2DShadow;", "")
                        .replace("precision mediump float;", "")
                };
                gl.shader_source(shader, &source);
                gl.compile_shader(shader);
                if !gl.get_shader_compile_status(shader) {
                    let error = gl.get_shader_info_log(shader);
                    gl.delete_shader(shader);
                    gl.delete_program(program);
                    return Err(format!("Turntable shader: {error}"));
                }
                gl.attach_shader(program, shader);
                gl.delete_shader(shader);
            }
            gl.link_program(program);
            if !gl.get_program_link_status(program) {
                let e = gl.get_program_info_log(program);
                gl.delete_program(program);
                return Err(e);
            }
            let mut uniforms = HashMap::new();
            for i in 0..gl.get_active_uniforms(program) {
                if let Some(info) = gl.get_active_uniform(program, i) {
                    let name = info.name.trim_end_matches("[0]");
                    if let Some(location) = gl.get_uniform_location(program, name) {
                        uniforms.insert(name.to_owned(), location);
                    }
                }
            }
            Ok(Self {
                handle: program,
                uniforms,
            })
        }
    }
    fn u(&self, name: &str) -> Option<&glow::UniformLocation> {
        self.uniforms.get(name)
    }
}

pub struct Scene {
    environment: Environment,
    room: Room,
    meshes: Meshes,
    standard: Program,
    record: Program,
    depth: Program,
    lut: glow::Texture,
    modulation: glow::Texture,
    label: glow::Texture,
    shadow: glow::Texture,
    shadow_fbo: glow::Framebuffer,
    light_vp: Mat4,
    camera: Option<Camera>,
    info: Option<RecordUniforms>,
}
impl Scene {
    pub fn new(gl: &glow::Context, root: &std::path::Path) -> Result<Self, String> {
        unsafe {
            let standard = Program::new(
                gl,
                include_str!("shaders/std_vs.glsl"),
                &format!(
                    "{}\n{}",
                    include_str!("shaders/pbr.glsl"),
                    include_str!("shaders/std_fs.glsl")
                ),
            )?;
            let record = Program::new(
                gl,
                include_str!("shaders/rec_vs.glsl"),
                &format!(
                    "{}\n{}",
                    include_str!("shaders/pbr.glsl"),
                    include_str!("shaders/rec_fs.glsl")
                ),
            )?;
            let depth = Program::new(
                gl,
                include_str!("shaders/std_vs.glsl"),
                include_str!("shaders/depth_fs.glsl"),
            )?;
            let meshes = Meshes::new(gl)?;
            let environment = Environment::new(gl)?;
            let room = Room::new(gl, &root.join("assets/room"))?;
            let lut = gl.create_texture()?;
            let modulation = gl.create_texture()?;
            let label = gl.create_texture()?;
            let shadow = gl.create_texture()?;
            gl.bind_texture(glow::TEXTURE_2D, Some(shadow));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::DEPTH_COMPONENT24 as i32,
                SHADOW,
                SHADOW,
                0,
                glow::DEPTH_COMPONENT,
                glow::UNSIGNED_INT,
                glow::PixelUnpackData::Slice(None),
            );
            for (p, v) in [
                (glow::TEXTURE_MIN_FILTER, glow::LINEAR),
                (glow::TEXTURE_MAG_FILTER, glow::LINEAR),
                (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE),
                (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
                (glow::TEXTURE_COMPARE_MODE, glow::COMPARE_REF_TO_TEXTURE),
                (glow::TEXTURE_COMPARE_FUNC, glow::LEQUAL),
            ] {
                gl.tex_parameter_i32(glow::TEXTURE_2D, p, v as i32);
            }
            let previous = NonZeroU32::new(gl.get_parameter_i32(glow::FRAMEBUFFER_BINDING) as u32)
                .map(glow::NativeFramebuffer);
            let shadow_fbo = gl.create_framebuffer()?;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(shadow_fbo));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::DEPTH_ATTACHMENT,
                glow::TEXTURE_2D,
                Some(shadow),
                0,
            );
            gl.draw_buffers(&[glow::NONE]);
            gl.read_buffer(glow::NONE);
            if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
                return Err("Shadow framebuffer is incomplete".into());
            }
            gl.bind_framebuffer(glow::FRAMEBUFFER, previous);
            let target = Vec3::new(0.04, 0.0, -0.07);
            let sun = environment.sun_direction();
            let up = if sun.dot(Vec3::Z).abs() > 0.98 {
                Vec3::Y
            } else {
                Vec3::Z
            };
            let light_vp = Mat4::orthographic_rh_gl(-0.9, 0.9, -0.8, 0.8, 0.1, 5.0)
                * Mat4::look_at_rh(target + sun * 2.5, target, up);
            let mut scene = Self {
                environment,
                room,
                meshes,
                standard,
                record,
                depth,
                lut,
                modulation,
                label,
                shadow,
                shadow_fbo,
                light_vp,
                camera: None,
                info: None,
            };
            scene.set_label_rgba(gl, 1024, 1024, &draw_label(&[]));
            Ok(scene)
        }
    }

    pub fn set_record(&mut self, gl: &glow::Context, visual: &RecordVisual) {
        let mut lut = [0.0_f32; 2048];
        for (i, value) in lut.iter_mut().enumerate() {
            let radius = 0.0512 + (0.1510 - 0.0512) * i as f32 / 2047.0;
            let table = &visual.r_table;
            if table.len() > 1 {
                let mut lo = 0;
                let mut hi = table.len() - 1;
                if radius >= table[0] {
                    *value = 0.0;
                    continue;
                }
                if radius <= table[hi] {
                    *value = hi as f32 / 64.0;
                    continue;
                }
                while hi - lo > 1 {
                    let mid = (hi + lo) / 2;
                    if table[mid] > radius {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                let span = table[lo] - table[hi];
                *value = (lo as f32
                    + if span > 0.0 {
                        (table[lo] - radius) / span
                    } else {
                        0.0
                    })
                    / 64.0;
            }
        }
        unsafe {
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.lut));
            let bytes: Vec<u8> = lut.iter().flat_map(|v| v.to_ne_bytes()).collect();
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::R32F as i32,
                2048,
                1,
                0,
                glow::RED,
                glow::FLOAT,
                glow::PixelUnpackData::Slice(Some(&bytes)),
            );
            texture_params(gl, glow::NEAREST, false);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.modulation));
            let valid = visual.cols > 0
                && visual.rows > 0
                && visual.modulation.len() == visual.cols * visual.rows;
            let (cols, rows, data) = if valid {
                (visual.cols, visual.rows, visual.modulation.as_slice())
            } else {
                (1, 1, &[0u8][..])
            };
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::R8 as i32,
                cols as i32,
                rows as i32,
                0,
                glow::RED,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(data)),
            );
            texture_params(gl, glow::LINEAR, true);
        }
        let mut warp = [0.0; 9];
        for (i, w) in visual.warp.iter().enumerate() {
            warp[i * 3..i * 3 + 3].copy_from_slice(w);
        }
        let mut scratches = [0.0; 48];
        for (i, s) in visual.scratches.iter().take(12).enumerate() {
            scratches[i * 4..i * 4 + 4].copy_from_slice(s);
        }
        self.info = Some(RecordUniforms {
            rows: visual.rows.max(1) as f32,
            rev_end: (visual.theta_end / std::f64::consts::TAU) as f32,
            warp,
            scratches,
            n_scratches: visual.scratches.len().min(12) as i32,
            dust: visual.dust,
            ecc: visual.ecc,
        });
        self.set_label_rgba(gl, 1024, 1024, &draw_label(&visual.titles));
    }

    /// Overrides the paper label with RGBA pixels, with the image's first row at the top.
    pub fn set_label_rgba(&mut self, gl: &glow::Context, width: i32, height: i32, pixels: &[u8]) {
        if width <= 0 || height <= 0 || pixels.len() != width as usize * height as usize * 4 {
            return;
        }
        let mut flipped = Vec::with_capacity(pixels.len());
        for row in pixels.chunks_exact(width as usize * 4).rev() {
            flipped.extend_from_slice(row);
        }
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(self.label));
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                width,
                height,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(&flipped)),
            );
            texture_params(gl, glow::LINEAR, false);
            gl.generate_mipmap(glow::TEXTURE_2D);
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR_MIPMAP_LINEAR as i32,
            );
        }
    }

    /// Pointer coordinates in normalized device space: left/bottom -1, right/top +1.
    pub fn pick(&self, ndc: [f32; 2], height: f32) -> Option<[f32; 2]> {
        let cam = self.camera.as_ref()?;
        let inv = cam.vp.inverse();
        let near = inv.project_point3(Vec3::new(ndc[0], ndc[1], -1.0));
        let far = inv.project_point3(Vec3::new(ndc[0], ndc[1], 1.0));
        let ray = far - near;
        if ray.z >= -1e-6 {
            return None;
        }
        let t = (height - near.z) / ray.z;
        (t >= 0.0).then(|| (near + ray * t).truncate().to_array())
    }
    pub fn arm_distance(point: [f32; 2], angle: f32) -> f32 {
        let p = Vec2::from_array(point);
        let mut best = p.distance(Vec2::from_array(stylus_xy(angle))) - 0.02;
        let rot = glam::Mat2::from_angle(angle);
        for q in arm_path().iter().skip(8) {
            best = best.min(p.distance(pivot().truncate() + rot * q.truncate()));
        }
        best
    }

    pub fn paint(&mut self, gl: &glow::Context, viewport: [i32; 4], state: &SceneState) {
        if viewport[2] <= 0 || viewport[3] <= 0 {
            return;
        }
        let aspect = viewport[2] as f32 / viewport[3] as f32;
        // Optical zoom keeps the panorama and foreground at the same magnification.
        // Moving toward the stylus on scroll creates parallax the panorama cannot supply.
        let stylus = Vec2::from_array(stylus_xy(state.arm_angle));
        let target = if state.inspect_stylus {
            Vec3::new(stylus.x, stylus.y, state.stylus_height)
        } else {
            Vec3::new(0.042, 0.08, 0.075)
        };
        let distance = if state.inspect_stylus { 0.12 } else { 0.95 };
        let framing = (1.5 / aspect).max(1.0);
        let fov = 2.0 * ((FOV * 0.5).tan() * framing * (-state.zoom).exp()).atan();
        let eye = target
            + distance
                * Vec3::new(
                    state.elevation.cos() * state.azimuth.sin(),
                    -state.elevation.cos() * state.azimuth.cos(),
                    state.elevation.sin(),
                );
        let vp = Mat4::perspective_rh_gl(fov, aspect, 0.004, 150.0)
            * Mat4::look_at_rh(eye, target, Vec3::Z);
        self.camera = Some(Camera { vp });
        let spin = Mat4::from_rotation_z(-state.platter_angle);
        let ecc = self.info.as_ref().map_or([0.0; 2], |r| r.ecc);
        let disc = spin * Mat4::from_translation(Vec3::new(ecc[0], ecc[1], 0.0));
        let arm = Mat4::from_translation(pivot())
            * Mat4::from_rotation_z(state.arm_angle)
            * Mat4::from_rotation_y(-(state.stylus_height / ARM_LE).clamp(-0.2, 0.2).asin());
        let head = arm
            * Mat4::from_translation(Vec3::new(ARM_LE, 0.0, 0.0))
            * Mat4::from_rotation_z(-beta());
        let lens = Mat4::from_translation(Vec3::new(
            -0.158 + 0.7026 * 0.0185,
            -0.160 + 0.7115 * 0.0185,
            0.0,
        )) * Mat4::from_rotation_z(0.7917);
        let on = if state.motor { 1.0 } else { 0.0 };
        let amber = |k: f32| [2.2 * k, 1.21 * k, 0.264 * k];
        let mut items = vec![
            Item::new("feet", Mat4::IDENTITY, BLACK, 0.8, 0.0, 0),
            Item::new("body", Mat4::IDENTITY, BLACK, 0.62, 0.0, 0),
            Item::new(
                "plate",
                Mat4::IDENTITY,
                [0.045, 0.047, 0.052],
                0.46,
                0.75,
                2,
            ),
            Item::new("platter", spin, [0.075, 0.075, 0.08], 0.36, 1.0, 3),
            Item::new("mat", spin, [0.010; 3], 0.9, 0.0, 0),
            Item::new("spindle", Mat4::IDENTITY, CHROME, 0.14, 1.0, 1),
            Item::new("strobe", Mat4::IDENTITY, [0.02, 0.02, 0.022], 0.4, 0.5, 0),
            Item::new("lens", lens, [0.15, 0.01, 0.01], 0.3, 0.0, 0).emissive([
                2.4 * on,
                0.16 * on,
                0.05 * on,
            ]),
            Item::new(
                "buttons",
                Mat4::IDENTITY,
                [0.028, 0.028, 0.030],
                0.42,
                0.3,
                0,
            ),
            Item::new("slot", Mat4::IDENTITY, [0.004; 3], 0.25, 0.0, 0),
            Item::new("ledStart", Mat4::IDENTITY, [0.05, 0.03, 0.01], 0.3, 0.0, 0)
                .emissive(amber(on)),
            Item::new("led33", Mat4::IDENTITY, [0.05, 0.03, 0.01], 0.3, 0.0, 0).emissive(amber(
                if state.rpm < 40.0 || state.rpm > 70.0 {
                    1.0
                } else {
                    0.0
                },
            )),
            Item::new("led45", Mat4::IDENTITY, [0.05, 0.03, 0.01], 0.3, 0.0, 0)
                .emissive(amber(if state.rpm > 40.0 { 1.0 } else { 0.0 })),
            Item::new("ledPitch", Mat4::IDENTITY, [0.01, 0.05, 0.02], 0.3, 0.0, 0)
                .emissive([0.44, 2.2, 0.77]),
            Item::new("tower", Mat4::IDENTITY, [0.020, 0.020, 0.022], 0.38, 0.6, 0),
            Item::new("towerCap", Mat4::IDENTITY, CHROME, 0.16, 1.0, 0),
            Item::new("tube", arm, [0.30, 0.30, 0.32], 0.16, 1.0, 0),
            Item::new("weight", arm, [0.022, 0.022, 0.024], 0.42, 0.7, 0),
            Item::new("dial", arm, CHROME, 0.28, 1.0, 0),
            Item::new("collar", head, CHROME, 0.2, 1.0, 0),
            Item::new("shell", head, [0.014, 0.014, 0.015], 0.4, 0.2, 0),
            Item::new("holder", head, [0.80, 0.36, 0.05], 0.42, 0.0, 0),
        ];
        let show_record = state.record && self.info.is_some();
        if show_record {
            items.push(Item::new("edge", disc, [0.008; 3], 0.22, 0.0, 0));
        }
        unsafe {
            let previous = NonZeroU32::new(gl.get_parameter_i32(glow::FRAMEBUFFER_BINDING) as u32)
                .map(glow::NativeFramebuffer);
            let scissor = gl.is_enabled(glow::SCISSOR_TEST);
            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::BLEND);
            gl.disable(glow::CULL_FACE);
            gl.enable(glow::DEPTH_TEST);
            gl.depth_func(glow::LESS);
            gl.depth_mask(true);
            gl.color_mask(true, true, true, true);
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.shadow_fbo));
            gl.viewport(0, 0, SHADOW, SHADOW);
            gl.clear_depth_f32(1.0);
            gl.clear(glow::DEPTH_BUFFER_BIT);
            gl.use_program(Some(self.depth.handle));
            matrix(gl, &self.depth, "uVP", self.light_vp);
            gl.enable(glow::POLYGON_OFFSET_FILL);
            gl.polygon_offset(2.0, 4.0);
            matrix(gl, &self.depth, "uM", Mat4::IDENTITY);
            for item in &items {
                matrix(gl, &self.depth, "uM", item.model);
                self.meshes.draw(gl, item.mesh);
            }
            if show_record {
                matrix(gl, &self.depth, "uM", disc);
                self.meshes.draw(gl, "disc");
            }
            gl.disable(glow::POLYGON_OFFSET_FILL);
            gl.bind_framebuffer(glow::FRAMEBUFFER, previous);
            gl.viewport(viewport[0], viewport[1], viewport[2], viewport[3]);
            // Restrict the clear to this widget, since egui also draws surrounding controls.
            gl.enable(glow::SCISSOR_TEST);
            gl.scissor(viewport[0], viewport[1], viewport[2], viewport[3]);
            gl.clear_color(0.65, 0.72, 0.78, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
            gl.active_texture(glow::TEXTURE3);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.shadow));
            self.room.draw(gl, vp, self.light_vp, self.shadow);
            let p = &self.standard;
            gl.use_program(Some(p.handle));
            self.common(gl, p, &eye, &vp, on);
            gl.uniform_2_f32(p.u("uPivot"), pivot().x, pivot().y);
            for item in &items {
                matrix(gl, p, "uM", item.model);
                gl.uniform_3_f32_slice(p.u("uCol"), &item.color);
                gl.uniform_3_f32_slice(p.u("uEmis"), &item.emission);
                gl.uniform_1_f32(p.u("uRough"), item.roughness);
                gl.uniform_1_f32(p.u("uMetal"), item.metal);
                gl.uniform_1_i32(p.u("uMode"), item.mode);
                self.meshes.draw(gl, item.mesh);
            }
            if let Some(info) = self.info.as_ref().filter(|_| show_record) {
                let p = &self.record;
                gl.use_program(Some(p.handle));
                self.common(gl, p, &eye, &vp, on);
                matrix(gl, p, "uM", disc);
                gl.uniform_3_f32_slice(p.u("uWarp"), &info.warp);
                gl.uniform_4_f32_slice(p.u("uScr"), &info.scratches);
                gl.uniform_1_i32(p.u("uNScr"), info.n_scratches);
                gl.uniform_1_f32(p.u("uRLo"), 0.0512);
                gl.uniform_1_f32(p.u("uRHi"), 0.1510);
                gl.uniform_1_f32(p.u("uRows"), info.rows);
                gl.uniform_1_f32(p.u("uRevEnd"), info.rev_end);
                gl.uniform_1_f32(p.u("uW"), 30e-6);
                gl.uniform_1_f32(p.u("uDust"), info.dust);
                for (i, (name, texture)) in [
                    ("uLut", self.lut),
                    ("uVis", self.modulation),
                    ("uLabel", self.label),
                ]
                .iter()
                .enumerate()
                {
                    gl.active_texture(glow::TEXTURE0 + i as u32);
                    gl.bind_texture(glow::TEXTURE_2D, Some(*texture));
                    gl.uniform_1_i32(p.u(name), i as i32);
                }
                self.meshes.draw(gl, "disc");
            }
            gl.bind_vertex_array(None);
            gl.use_program(None);
            gl.disable(glow::DEPTH_TEST);
            if !scissor {
                gl.disable(glow::SCISSOR_TEST);
            }
            gl.active_texture(glow::TEXTURE0);
        }
    }
    unsafe fn common(&self, gl: &glow::Context, p: &Program, eye: &Vec3, vp: &Mat4, on: f32) {
        unsafe {
            matrix(gl, p, "uVP", *vp);
            matrix(gl, p, "uLVP", self.light_vp);
            gl.uniform_3_f32_slice(p.u("uEye"), &eye.to_array());
            gl.uniform_1_i32(p.u("uShadow"), 3);
            self.environment.bind_lighting(gl, p.handle);
            gl.uniform_4_f32(
                p.u("uLamp"),
                -0.158 + 0.7026 * 0.024,
                -0.160 + 0.7115 * 0.024,
                -0.013,
                on,
            );
        }
    }
    pub fn destroy(&self, gl: &glow::Context) {
        unsafe {
            self.environment.destroy(gl);
            self.room.destroy(gl);
            self.meshes.destroy(gl);
            for program in [&self.standard, &self.record, &self.depth] {
                gl.delete_program(program.handle);
            }
            for texture in [self.lut, self.modulation, self.label, self.shadow] {
                gl.delete_texture(texture);
            }
            gl.delete_framebuffer(self.shadow_fbo);
        }
    }
}
struct Item {
    mesh: &'static str,
    model: Mat4,
    color: [f32; 3],
    roughness: f32,
    metal: f32,
    mode: i32,
    emission: [f32; 3],
}
impl Item {
    fn new(
        mesh: &'static str,
        model: Mat4,
        color: [f32; 3],
        roughness: f32,
        metal: f32,
        mode: i32,
    ) -> Self {
        Self {
            mesh,
            model,
            color,
            roughness,
            metal,
            mode,
            emission: [0.0; 3],
        }
    }
    fn emissive(mut self, emission: [f32; 3]) -> Self {
        self.emission = emission;
        self
    }
}
unsafe fn matrix(gl: &glow::Context, p: &Program, name: &str, value: Mat4) {
    unsafe {
        gl.uniform_matrix_4_f32_slice(p.u(name), false, &value.to_cols_array());
    }
}
unsafe fn texture_params(gl: &glow::Context, filter: u32, repeat: bool) {
    unsafe {
        for (p, v) in [
            (glow::TEXTURE_MIN_FILTER, filter),
            (glow::TEXTURE_MAG_FILTER, filter),
            (
                glow::TEXTURE_WRAP_S,
                if repeat {
                    glow::REPEAT
                } else {
                    glow::CLAMP_TO_EDGE
                },
            ),
            (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
        ] {
            gl.tex_parameter_i32(glow::TEXTURE_2D, p, v as i32);
        }
    }
}
fn arm_path() -> Vec<Vec3> {
    let b = beta();
    let p0 = Vec3::ZERO;
    let p1 = Vec3::new(0.075 * 0.28_f32.cos(), 0.075 * 0.28_f32.sin(), -0.003);
    let p3 = Vec3::new(ARM_LE - 0.040 * b.cos(), 0.040 * b.sin(), -0.0125);
    let p2 = Vec3::new(p3.x - 0.075 * b.cos(), p3.y + 0.075 * b.sin(), -0.0085);
    (0..=32)
        .map(|i| {
            let t = i as f32 / 32.0;
            let u = 1.0 - t;
            p0 * u * u * u + p1 * 3.0 * u * u * t + p2 * 3.0 * u * t * t + p3 * t * t * t
        })
        .collect()
}

// The label is rasterized entirely in Rust. It preserves the original dark paper,
// engraved rings, type, grain and worn ink without browser canvases.
fn draw_label(titles: &[String]) -> Vec<u8> {
    const S: usize = 1024;
    let mut pixels = vec![0u8; S * S * 4];
    let mut seed = 41u32;
    let mut random = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 8) as f32 / 16777216.0
    };
    for y in 0..S {
        for x in 0..S {
            let dx = x as f32 - 512.0;
            let dy = y as f32 - 512.0;
            let r = dx.hypot(dy);
            let mottle = (x as f32 * 0.019).sin() * (y as f32 * 0.014).cos() * 1.2
                + (x as f32 * 0.071 + y as f32 * 0.057).sin() * 0.4;
            let base = (30.4 + mottle + (random() + random() - 1.0) * 6.0)
                * (1.0 - 0.3 * ((r / 512.0 - 0.4) / 0.6).clamp(0.0, 1.0).powi(3));
            let at = (y * S + x) * 4;
            pixels[at] = base as u8;
            pixels[at + 1] = base as u8;
            pixels[at + 2] = (base + 0.8) as u8;
            pixels[at + 3] = 255;
            for (radius, width, opacity) in [(489.0, 3.0, 0.92), (440.0, 1.2, 0.9)] {
                let alpha = (width * 0.5 + 0.5 - (r - radius).abs()).clamp(0.0, 1.0) * opacity;
                blend(
                    &mut pixels,
                    x as i32,
                    y as i32,
                    [113, 109, 100],
                    alpha * (0.7 + random() * 0.3),
                );
            }
        }
    }
    // Fine fibres and circular scuffs sit in the paper and pass through the ink.
    for _ in 0..1500 {
        let x = random() * 1024.0;
        let y = random() * 1024.0;
        let len = 5.0 + random() * 30.0;
        let angle = random() * TAU;
        for i in 0..len as usize {
            blend(
                &mut pixels,
                (x + i as f32 * angle.cos()) as i32,
                (y + i as f32 * angle.sin()) as i32,
                [75, 72, 67],
                0.08,
            );
        }
    }
    let fonts = egui::FontDefinitions::default();
    let font_data = fonts
        .font_data
        .get("Hack")
        .or_else(|| fonts.font_data.values().next());
    if let Some(font) = font_data.and_then(|data| {
        fontdue::Font::from_bytes(data.font.as_ref(), fontdue::FontSettings::default()).ok()
    }) {
        label_text(&mut pixels, &font, "STEREO", 23.0, 212.0, 503.0, 200.0);
        label_text(&mut pixels, &font, "SIDE A", 23.0, 212.0, 537.0, 200.0);
        label_text(&mut pixels, &font, "33⅓", 23.0, 812.0, 503.0, 200.0);
        label_text(&mut pixels, &font, "RPM", 23.0, 812.0, 537.0, 200.0);
        for (i, title) in titles.iter().filter(|t| !t.is_empty()).take(5).enumerate() {
            let dy = 204.0 + i as f32 * 40.0;
            let max_width = 2.0 * (425.0 * 425.0 - dy * dy).sqrt() - 44.0;
            let text = format!("{}. {}", i + 1, title)
                .chars()
                .take(40)
                .collect::<String>();
            label_text(
                &mut pixels,
                &font,
                &text,
                27.0,
                512.0,
                512.0 + dy + 10.0,
                max_width,
            );
        }
        let text = "NOT A SOUND EFFECT  ·  CUT INTO A GROOVE  ·  READ BACK BY A SIMULATED STYLUS";
        let advances: Vec<f32> = text
            .chars()
            .map(|ch| font.metrics(ch, 15.0).advance_width + 5.0)
            .collect();
        let mut angle = PI / 2.0 + advances.iter().sum::<f32>() / 464.0 / 2.0;
        for (ch, advance) in text.chars().zip(advances) {
            angle -= advance / 464.0 / 2.0;
            let (metrics, bitmap) = font.rasterize(ch, 15.0);
            let center = Vec2::new(512.0 + 464.0 * angle.cos(), 512.0 + 464.0 * angle.sin());
            let rotation = glam::Mat2::from_angle(angle - PI / 2.0);
            for gy in 0..metrics.height {
                for gx in 0..metrics.width {
                    let local = Vec2::new(
                        gx as f32 - metrics.width as f32 / 2.0,
                        gy as f32 - metrics.height as f32 - metrics.ymin as f32,
                    );
                    let p = center + rotation * local;
                    blend(
                        &mut pixels,
                        p.x as i32,
                        p.y as i32,
                        [113, 109, 100],
                        bitmap[gy * metrics.width + gx] as f32 / 255.0 * 0.86,
                    );
                }
            }
            angle -= advance / 464.0 / 2.0;
        }
    }
    for y in 345..679 {
        for x in 345..679 {
            let r = (x as f32 - 512.0).hypot(y as f32 - 512.0);
            blend(
                &mut pixels,
                x,
                y,
                [0, 0, 0],
                (2.25 - (r - 158.0).abs()).clamp(0.0, 1.0) * 0.3,
            );
            blend(
                &mut pixels,
                x,
                y,
                [70, 68, 64],
                (1.5 - (r - 162.0).abs()).clamp(0.0, 1.0) * 0.12,
            );
        }
    }
    pixels
}
fn label_text(
    pixels: &mut [u8],
    font: &fontdue::Font,
    text: &str,
    mut size: f32,
    cx: f32,
    baseline: f32,
    max_width: f32,
) {
    let width: f32 = text
        .chars()
        .map(|ch| font.metrics(ch, size).advance_width)
        .sum();
    if width > max_width {
        size *= max_width / width;
    }
    let width: f32 = text
        .chars()
        .map(|ch| font.metrics(ch, size).advance_width)
        .sum();
    let mut x = cx - width / 2.0;
    for ch in text.chars() {
        let (metrics, bitmap) = font.rasterize(ch, size);
        let top = baseline - metrics.height as f32 - metrics.ymin as f32;
        for gy in 0..metrics.height {
            for gx in 0..metrics.width {
                blend(
                    pixels,
                    x as i32 + metrics.xmin + gx as i32,
                    top as i32 + gy as i32,
                    [113, 109, 100],
                    bitmap[gy * metrics.width + gx] as f32 / 255.0 * 0.88,
                );
            }
        }
        x += metrics.advance_width;
    }
}
fn blend(pixels: &mut [u8], x: i32, y: i32, color: [u8; 3], alpha: f32) {
    if !(0..1024).contains(&x) || !(0..1024).contains(&y) {
        return;
    }
    let i = (y as usize * 1024 + x as usize) * 4;
    for c in 0..3 {
        pixels[i + c] = (pixels[i + c] as f32 * (1.0 - alpha) + color[c] as f32 * alpha) as u8;
    }
}
