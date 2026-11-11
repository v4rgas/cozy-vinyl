//! Lighting sampled from the furnished room at the record player.
use eframe::glow::{self, HasContext};
use glam::{Mat4, Vec3};
use serde::Deserialize;
const ENVIRONMENT_ROTATION: f32 = 0.0;
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
pub struct Environment {
    environment: glow::Texture,
    prefilter: glow::Texture,
    brdf: glow::Texture,
    lighting: Lighting,
}
impl Environment {
    pub fn new(gl: &glow::Context) -> Result<Self, String> {
        unsafe {
            let lighting: Lighting =
                serde_json::from_str(include_str!("../../../assets/room/lighting.json"))
                    .map_err(|e| e.to_string())?;
            let hdr = image::load_from_memory(include_bytes!("../../../assets/room/balcony.hdr"))
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
            let specular = include_bytes!("../../../assets/room/specular.bin");
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
                glow::PixelUnpackData::Slice(Some(include_bytes!("../../../assets/room/brdf.bin"))),
            );
            texture_parameters(gl, false);
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            Ok(Self {
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
    pub fn destroy(&self, gl: &glow::Context) {
        unsafe {
            for texture in [self.environment, self.prefilter, self.brdf] {
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
