//! The apartment's baked, unlit materials retain their original display colors.
use eframe::glow::{self, HasContext};
use glam::Mat4;
use serde::Deserialize;
use std::{fs, path::Path};

#[derive(Deserialize)]
struct Mesh {
    first: i32,
    count: i32,
    material: usize,
}
#[derive(Deserialize)]
struct Material {
    texture: String,
    #[serde(default)]
    double_sided: bool,
}
#[derive(Deserialize)]
struct Manifest {
    meshes: Vec<Mesh>,
    materials: Vec<Material>,
}

pub struct Room {
    program: glow::Program,
    vao: glow::VertexArray,
    buffer: glow::Buffer,
    textures: Vec<glow::Texture>,
    manifest: Manifest,
    vp: Option<glow::UniformLocation>,
    light_vp: Option<glow::UniformLocation>,
}

impl Room {
    pub fn new(gl: &glow::Context, directory: &Path) -> Result<Self, String> {
        let manifest: Manifest = serde_json::from_slice(
            &fs::read(directory.join("manifest.json"))
                .map_err(|e| format!("Room manifest: {e}"))?,
        )
        .map_err(|e| format!("Room manifest: {e}"))?;
        let vertices =
            fs::read(directory.join("scene.bin")).map_err(|e| format!("Room geometry: {e}"))?;
        if vertices.len() % 32 != 0
            || manifest.meshes.iter().any(|mesh| {
                mesh.first < 0
                    || mesh.count < 0
                    || mesh.count % 3 != 0
                    || mesh.first as i64 + mesh.count as i64 > (vertices.len() / 32) as i64
                    || mesh.material >= manifest.materials.len()
            })
        {
            return Err("Invalid room mesh ranges or vertex data".into());
        }
        unsafe {
            let program = program(gl)?;
            let vao = gl.create_vertex_array().map_err(|e| {
                gl.delete_program(program);
                e
            })?;
            let buffer = gl.create_buffer().map_err(|e| {
                gl.delete_vertex_array(vao);
                gl.delete_program(program);
                e
            })?;
            let mut room = Self {
                program,
                vao,
                buffer,
                textures: Vec::new(),
                manifest,
                vp: gl.get_uniform_location(program, "uVP"),
                light_vp: gl.get_uniform_location(program, "uLVP"),
            };
            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, &vertices, glow::STATIC_DRAW);
            for (location, size, offset) in [(0, 3, 0), (1, 3, 12), (2, 2, 24)] {
                gl.enable_vertex_attrib_array(location);
                gl.vertex_attrib_pointer_f32(location, size, glow::FLOAT, false, 32, offset);
            }
            gl.bind_vertex_array(None);
            for material in &room.manifest.materials {
                let result = texture(gl, &directory.join(&material.texture));
                match result {
                    Ok(texture) => room.textures.push(texture),
                    Err(error) => {
                        room.destroy(gl);
                        return Err(error);
                    }
                }
            }
            gl.use_program(Some(program));
            gl.uniform_1_i32(gl.get_uniform_location(program, "uTexture").as_ref(), 0);
            gl.uniform_1_i32(gl.get_uniform_location(program, "uShadow").as_ref(), 3);
            Ok(room)
        }
    }

    pub unsafe fn draw(&self, gl: &glow::Context, vp: Mat4, light_vp: Mat4, shadow: glow::Texture) {
        unsafe {
            gl.use_program(Some(self.program));
            gl.uniform_matrix_4_f32_slice(self.vp.as_ref(), false, &vp.to_cols_array());
            gl.uniform_matrix_4_f32_slice(self.light_vp.as_ref(), false, &light_vp.to_cols_array());
            gl.active_texture(glow::TEXTURE3);
            gl.bind_texture(glow::TEXTURE_2D, Some(shadow));
            gl.active_texture(glow::TEXTURE0);
            self.draw_meshes(gl);
        }
    }

    unsafe fn draw_meshes(&self, gl: &glow::Context) {
        unsafe {
            let cull_enabled = gl.is_enabled(glow::CULL_FACE);
            let cull_mode = gl.get_parameter_i32(glow::CULL_FACE_MODE) as u32;
            gl.cull_face(glow::BACK);
            gl.bind_vertex_array(Some(self.vao));
            for mesh in &self.manifest.meshes {
                if self.manifest.materials[mesh.material].double_sided {
                    gl.disable(glow::CULL_FACE);
                } else {
                    gl.enable(glow::CULL_FACE);
                }
                gl.bind_texture(glow::TEXTURE_2D, Some(self.textures[mesh.material]));
                gl.draw_arrays(glow::TRIANGLES, mesh.first, mesh.count);
            }
            gl.cull_face(cull_mode);
            if cull_enabled {
                gl.enable(glow::CULL_FACE);
            } else {
                gl.disable(glow::CULL_FACE);
            }
        }
    }

    pub fn destroy(&self, gl: &glow::Context) {
        unsafe {
            gl.delete_program(self.program);
            gl.delete_vertex_array(self.vao);
            gl.delete_buffer(self.buffer);
            for &texture in &self.textures {
                gl.delete_texture(texture);
            }
        }
    }
}

unsafe fn program(gl: &glow::Context) -> Result<glow::Program, String> {
    unsafe {
        let program = gl.create_program()?;
        let result = (|| {
            for (kind, source) in [
                (glow::VERTEX_SHADER, include_str!("shaders/room_vs.glsl")),
                (glow::FRAGMENT_SHADER, include_str!("shaders/room_fs.glsl")),
            ] {
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
                    let error = gl.get_shader_info_log(shader);
                    gl.delete_shader(shader);
                    return Err(error);
                }
                gl.attach_shader(program, shader);
                gl.delete_shader(shader);
            }
            gl.link_program(program);
            if gl.get_program_link_status(program) {
                Ok(program)
            } else {
                Err(gl.get_program_info_log(program))
            }
        })();
        if result.is_err() {
            gl.delete_program(program);
        }
        result
    }
}

unsafe fn texture(gl: &glow::Context, path: &Path) -> Result<glow::Texture, String> {
    let image = image::open(path)
        .map_err(|e| format!("Room texture {}: {e}", path.display()))?
        .to_rgba8();
    unsafe {
        let texture = gl.create_texture()?;
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::SRGB8_ALPHA8 as i32,
            image.width() as i32,
            image.height() as i32,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(Some(image.as_raw())),
        );
        gl.generate_mipmap(glow::TEXTURE_2D);
        for (parameter, value) in [
            (glow::TEXTURE_MIN_FILTER, glow::LINEAR_MIPMAP_LINEAR),
            (glow::TEXTURE_MAG_FILTER, glow::LINEAR),
            (glow::TEXTURE_WRAP_S, glow::REPEAT),
            (glow::TEXTURE_WRAP_T, glow::REPEAT),
        ] {
            gl.tex_parameter_i32(glow::TEXTURE_2D, parameter, value as i32);
        }
        if gl
            .supported_extensions()
            .contains("GL_EXT_texture_filter_anisotropic")
        {
            let maximum = gl.get_parameter_f32(glow::MAX_TEXTURE_MAX_ANISOTROPY_EXT);
            gl.tex_parameter_f32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAX_ANISOTROPY_EXT,
                maximum.min(8.),
            );
        }
        Ok(texture)
    }
}
