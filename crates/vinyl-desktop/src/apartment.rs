//! The selected photographic environment and scanned wooden table share one
//! linear HDR light field with the turntable. Lighting is baked offline in Rust.
use eframe::glow::{self, HasContext};
use glam::{Mat4, Vec3};
use serde::Deserialize;
use std::collections::HashMap;

pub const ENVIRONMENT_ROTATION: f32 = 0.105;
#[derive(Deserialize)]
struct Sun {
    direction: [f32; 3],
    radiance: [f32; 3],
}
#[derive(Deserialize)]
struct Level {
    width: i32,
    height: i32,
    offset_bytes: usize,
}
#[derive(Deserialize)]
struct Specular {
    levels: i32,
    offsets: Vec<Level>,
}
#[derive(Deserialize)]
struct Brdf {
    width: i32,
    height: i32,
}
#[derive(Deserialize)]
struct Lighting {
    exposure: f32,
    sh9: [[f32; 3]; 9],
    sun: Sun,
    specular: Specular,
    brdf: Brdf,
}
struct Shader {
    program: glow::Program,
    uniforms: HashMap<String, glow::UniformLocation>,
}
impl Shader {
    unsafe fn new(gl: &glow::Context, vs: &str, fs: &str) -> Result<Self, String> {
        unsafe {
            let program = gl.create_program()?;
            for (kind, source) in [(glow::VERTEX_SHADER, vs), (glow::FRAGMENT_SHADER, fs)] {
                let shader = gl.create_shader(kind)?;
                let source = if gl.version().is_embedded {
                    source.to_owned()
                } else {
                    source
                        .replace("#version 300 es", "#version 330 core")
                        .replace("precision highp float;", "")
                        .replace("precision highp sampler2DShadow;", "")
                };
                gl.shader_source(shader, &source);
                gl.compile_shader(shader);
                if !gl.get_shader_compile_status(shader) {
                    let e = gl.get_shader_info_log(shader);
                    gl.delete_shader(shader);
                    gl.delete_program(program);
                    return Err(e);
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
                if let Some(u) = gl.get_active_uniform(program, i) {
                    if let Some(location) = gl.get_uniform_location(program, &u.name) {
                        uniforms.insert(u.name, location);
                    }
                }
            }
            Ok(Self { program, uniforms })
        }
    }
    fn u(&self, name: &str) -> Option<&glow::UniformLocation> {
        self.uniforms.get(name)
    }
}
pub struct Apartment {
    surface: Shader,
    sky: Shader,
    table_vao: glow::VertexArray,
    table_buffer: glow::Buffer,
    table_count: i32,
    sky_vao: glow::VertexArray,
    sky_buffer: glow::Buffer,
    base: glow::Texture,
    normal: glow::Texture,
    arm: glow::Texture,
    environment: glow::Texture,
    prefilter: glow::Texture,
    brdf: glow::Texture,
    lighting: Lighting,
}
impl Apartment {
    pub fn new(gl: &glow::Context) -> Result<Self, String> {
        unsafe {
            let lighting: Lighting =
                serde_json::from_str(include_str!("../../../assets/apartment/lighting.json"))
                    .map_err(|e| e.to_string())?;
            let common = include_str!("shaders/pbr.glsl");
            let surface = Shader::new(
                gl,
                include_str!("shaders/apartment_vs.glsl"),
                &format!("{common}\n{}", include_str!("shaders/apartment_fs.glsl")),
            )?;
            let sky = Shader::new(
                gl,
                include_str!("shaders/apartment_sky_vs.glsl"),
                &format!(
                    "{common}\n{}",
                    include_str!("shaders/apartment_sky_fs.glsl")
                ),
            )?;
            let base = material_texture(
                gl,
                include_bytes!("../../../assets/apartment/table.jpg"),
                true,
            )?;
            let normal = material_texture(
                gl,
                include_bytes!("../../../assets/apartment/table_normal.jpg"),
                false,
            )?;
            let arm = material_texture(
                gl,
                include_bytes!("../../../assets/apartment/table_arm.jpg"),
                false,
            )?;
            let hdr =
                image::load_from_memory(include_bytes!("../../../assets/apartment/balcony.hdr"))
                    .map_err(|e| e.to_string())?
                    .to_rgb32f();
            let pixels: Vec<u8> = hdr.as_raw().iter().flat_map(|v| v.to_ne_bytes()).collect();
            let environment = gl.create_texture()?;
            gl.bind_texture(glow::TEXTURE_2D, Some(environment));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGB32F as i32,
                hdr.width() as i32,
                hdr.height() as i32,
                0,
                glow::RGB,
                glow::FLOAT,
                glow::PixelUnpackData::Slice(Some(&pixels)),
            );
            gl.generate_mipmap(glow::TEXTURE_2D);
            texture_parameters(gl, true);
            let prefilter = gl.create_texture()?;
            gl.bind_texture(glow::TEXTURE_2D, Some(prefilter));
            let specular = include_bytes!("../../../assets/apartment/specular.bin");
            for (level, spec) in lighting.specular.offsets.iter().enumerate() {
                let end = spec.offset_bytes + spec.width as usize * spec.height as usize * 12;
                let pixels = specular
                    .get(spec.offset_bytes..end)
                    .ok_or("Invalid prefiltered environment data")?;
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    level as i32,
                    glow::RGB16F as i32,
                    spec.width,
                    spec.height,
                    0,
                    glow::RGB,
                    glow::FLOAT,
                    glow::PixelUnpackData::Slice(Some(pixels)),
                );
            }
            texture_parameters(gl, true);
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAX_LEVEL,
                lighting.specular.levels - 1,
            );
            let brdf = gl.create_texture()?;
            gl.bind_texture(glow::TEXTURE_2D, Some(brdf));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RG16F as i32,
                lighting.brdf.width,
                lighting.brdf.height,
                0,
                glow::RG,
                glow::FLOAT,
                glow::PixelUnpackData::Slice(Some(include_bytes!(
                    "../../../assets/apartment/brdf.bin"
                ))),
            );
            texture_parameters(gl, false);
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            let table_vao = gl.create_vertex_array()?;
            let table_buffer = gl.create_buffer()?;
            let table = include_bytes!("../../../assets/apartment/table.bin");
            gl.bind_vertex_array(Some(table_vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(table_buffer));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, table, glow::STATIC_DRAW);
            for (location, size, offset) in [(0, 3, 0), (1, 3, 12), (2, 2, 24)] {
                gl.enable_vertex_attrib_array(location);
                gl.vertex_attrib_pointer_f32(location, size, glow::FLOAT, false, 32, offset);
            }
            let sky_vao = gl.create_vertex_array()?;
            let sky_buffer = gl.create_buffer()?;
            gl.bind_vertex_array(Some(sky_vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(sky_buffer));
            let vertices: [f32; 9] = [-1., -1., 0., 3., -1., 0., -1., 3., 0.];
            let bytes: Vec<u8> = vertices.iter().flat_map(|v| v.to_ne_bytes()).collect();
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, &bytes, glow::STATIC_DRAW);
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, 12, 0);
            gl.bind_vertex_array(None);
            Ok(Self {
                surface,
                sky,
                table_vao,
                table_buffer,
                table_count: (table.len() / 32) as i32,
                sky_vao,
                sky_buffer,
                base,
                normal,
                arm,
                environment,
                prefilter,
                brdf,
                lighting,
            })
        }
    }
    pub fn sun_direction(&self) -> Vec3 {
        Mat4::from_rotation_z(-ENVIRONMENT_ROTATION * std::f32::consts::TAU)
            .transform_vector3(Vec3::from_array(self.lighting.sun.direction))
            .normalize()
    }
    /// Binds the same measured light field for every surface and the background.
    pub unsafe fn bind_lighting(&self, gl: &glow::Context, program: glow::Program) {
        unsafe {
            let u = |name: &str| gl.get_uniform_location(program, name);
            gl.uniform_1_f32(u("uExposure").as_ref(), self.lighting.exposure);
            gl.uniform_1_f32(u("uEnvironmentRotation").as_ref(), ENVIRONMENT_ROTATION);
            gl.uniform_1_f32(
                u("uSpecularLevels").as_ref(),
                self.lighting.specular.levels as f32,
            );
            gl.uniform_3_f32_slice(
                u("uSunDirection").as_ref(),
                &self.sun_direction().to_array(),
            );
            gl.uniform_3_f32_slice(u("uSunRadiance").as_ref(), &self.lighting.sun.radiance);
            let sh: Vec<f32> = self.lighting.sh9.iter().flatten().copied().collect();
            gl.uniform_3_f32_slice(u("uSH").as_ref(), &sh);
            for (unit, name, texture) in [
                (4, "uEnvironment", self.environment),
                (7, "uPrefilter", self.prefilter),
                (8, "uBRDF", self.brdf),
            ] {
                gl.active_texture(glow::TEXTURE0 + unit);
                gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                gl.uniform_1_i32(u(name).as_ref(), unit as i32);
            }
        }
    }
    pub unsafe fn shadow(&self, gl: &glow::Context) {
        unsafe {
            gl.bind_vertex_array(Some(self.table_vao));
            gl.draw_arrays(glow::TRIANGLES, 0, self.table_count);
        }
    }
    pub unsafe fn draw(
        &self,
        gl: &glow::Context,
        vp: Mat4,
        eye: Vec3,
        light_vp: Mat4,
        shadow: glow::Texture,
    ) {
        unsafe {
            gl.disable(glow::DEPTH_TEST);
            gl.depth_mask(false);
            gl.use_program(Some(self.sky.program));
            self.bind_lighting(gl, self.sky.program);
            gl.uniform_matrix_4_f32_slice(
                self.sky.u("uInverseVP"),
                false,
                &vp.inverse().to_cols_array(),
            );
            gl.uniform_3_f32_slice(self.sky.u("uEye"), &eye.to_array());
            gl.bind_vertex_array(Some(self.sky_vao));
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            gl.depth_mask(true);
            gl.enable(glow::DEPTH_TEST);
            gl.use_program(Some(self.surface.program));
            self.bind_lighting(gl, self.surface.program);
            gl.uniform_matrix_4_f32_slice(self.surface.u("uVP"), false, &vp.to_cols_array());
            gl.uniform_matrix_4_f32_slice(self.surface.u("uLVP"), false, &light_vp.to_cols_array());
            gl.uniform_3_f32_slice(self.surface.u("uEye"), &eye.to_array());
            gl.uniform_1_i32(self.surface.u("uShadow"), 3);
            for (unit, name, texture) in [
                (3, "uShadow", shadow),
                (5, "uTexture", self.base),
                (6, "uNormal", self.normal),
                (9, "uARM", self.arm),
            ] {
                gl.active_texture(glow::TEXTURE0 + unit);
                gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                gl.uniform_1_i32(self.surface.u(name), unit as i32);
            }
            gl.bind_vertex_array(Some(self.table_vao));
            gl.draw_arrays(glow::TRIANGLES, 0, self.table_count);
        }
    }
    pub fn destroy(&self, gl: &glow::Context) {
        unsafe {
            gl.delete_vertex_array(self.table_vao);
            gl.delete_vertex_array(self.sky_vao);
            gl.delete_buffer(self.table_buffer);
            gl.delete_buffer(self.sky_buffer);
            gl.delete_program(self.surface.program);
            gl.delete_program(self.sky.program);
            for texture in [
                self.base,
                self.normal,
                self.arm,
                self.environment,
                self.prefilter,
                self.brdf,
            ] {
                gl.delete_texture(texture);
            }
        }
    }
}
unsafe fn texture_parameters(gl: &glow::Context, mipmaps: bool) {
    unsafe {
        for (p, v) in [
            (
                glow::TEXTURE_MIN_FILTER,
                if mipmaps {
                    glow::LINEAR_MIPMAP_LINEAR
                } else {
                    glow::LINEAR
                },
            ),
            (glow::TEXTURE_MAG_FILTER, glow::LINEAR),
            (glow::TEXTURE_WRAP_S, glow::REPEAT),
            (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
        ] {
            gl.tex_parameter_i32(glow::TEXTURE_2D, p, v as i32);
        }
    }
}
unsafe fn material_texture(
    gl: &glow::Context,
    bytes: &[u8],
    srgb: bool,
) -> Result<glow::Texture, String> {
    unsafe {
        let image = image::load_from_memory(bytes)
            .map_err(|e| e.to_string())?
            .to_rgba8();
        let texture = gl.create_texture()?;
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            if srgb {
                glow::SRGB8_ALPHA8
            } else {
                glow::RGBA8
            } as i32,
            image.width() as i32,
            image.height() as i32,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(Some(image.as_raw())),
        );
        gl.generate_mipmap(glow::TEXTURE_2D);
        texture_parameters(gl, true);
        if gl
            .supported_extensions()
            .contains("GL_EXT_texture_filter_anisotropic")
        {
            let max = gl.get_parameter_f32(glow::MAX_TEXTURE_MAX_ANISOTROPY_EXT);
            gl.tex_parameter_f32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAX_ANISOTROPY_EXT,
                max.min(8.),
            );
        }
        Ok(texture)
    }
}
