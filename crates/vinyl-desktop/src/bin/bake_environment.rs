//! Reproducible, offline HDR image based lighting bake.
//!
//! Run from the repository root:
//!   cargo run --release -p vinyl-desktop --bin bake_environment --offline
//!
//! The SH stores irradiance / pi, with normalized real SH basis ordered
//! [1,y,z,x,xy,yz,3z²-1,xz,x²-y²]. Lambert convolution multipliers are
//! [1,2/3,2/3,2/3,1/4,1/4,1/4,1/4,1/4]. Evaluate the basis again at N.
//! All radiometry remains scene-linear, with no white balance or exposure baked in.
//! Sun RGB is the integral of removed excess radiance over solid angle; multiply
//! this directional irradiance by the surface BRDF and N dot L at runtime.
//!
//! Split-sum GGX follows LearnOpenGL PBR/IBL/Specular-IBL and Khronos'
//! glTF-Sample-Renderer IBL filtering: alpha=perceptual_roughness²; V=N for
//! prefiltering; the BRDF LUT integrates Schlick-GGX visibility (k=roughness²/2).
//! Runtime specular = prefiltered(R, roughness) * (F0 * lut.r + lut.g).
use glam::{DVec3, Vec2, Vec3};
use image::ImageReader;
use serde_json::json;
use std::{
    error::Error,
    f64::consts::{PI, TAU},
    fs::{self, File},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    time::Instant,
};

const PREFILTER_WIDTH: usize = 512;
const PREFILTER_HEIGHT: usize = 256;
const PREFILTER_LEVELS: usize = 10;
const ENV_SAMPLES: u32 = 256;
const LUT_SIZE: usize = 128;
const LUT_SAMPLES: u32 = 256;

#[derive(Clone)]
struct Level {
    width: usize,
    height: usize,
    pixels: Vec<Vec3>,
}
impl Level {
    fn sample(&self, dir: Vec3) -> Vec3 {
        let u = dir.y.atan2(dir.x) / std::f32::consts::TAU + 0.5;
        let v = dir.z.clamp(-1.0, 1.0).acos() / std::f32::consts::PI;
        let x = u * self.width as f32 - 0.5;
        let y = v * self.height as f32 - 0.5;
        let ix = x.floor() as isize;
        let iy = y.floor() as isize;
        let fx = x - x.floor();
        let fy = y - y.floor();
        let get = |x: isize, y: isize| {
            self.pixels[y.clamp(0, self.height as isize - 1) as usize * self.width
                + x.rem_euclid(self.width as isize) as usize]
        };
        get(ix, iy)
            .lerp(get(ix + 1, iy), fx)
            .lerp(get(ix, iy + 1).lerp(get(ix + 1, iy + 1), fx), fy)
    }
}
struct Environment {
    levels: Vec<Level>,
}
impl Environment {
    fn new(base: Level) -> Self {
        let mut levels = vec![base];
        while levels.last().unwrap().width > 1 || levels.last().unwrap().height > 1 {
            let src = levels.last().unwrap();
            let width = (src.width / 2).max(1);
            let height = (src.height / 2).max(1);
            let mut pixels = vec![Vec3::ZERO; width * height];
            for y in 0..height {
                for x in 0..width {
                    let mut sum = DVec3::ZERO;
                    let mut weight = 0.0;
                    for dy in 0..if src.height > 1 { 2 } else { 1 } {
                        let sy = (y * 2 + dy).min(src.height - 1);
                        let dw = row_solid_angle(src.width, src.height, sy);
                        for dx in 0..if src.width > 1 { 2 } else { 1 } {
                            let sx = (x * 2 + dx).min(src.width - 1);
                            sum += src.pixels[sy * src.width + sx].as_dvec3() * dw;
                            weight += dw;
                        }
                    }
                    pixels[y * width + x] = (sum / weight).as_vec3();
                }
            }
            levels.push(Level {
                width,
                height,
                pixels,
            });
        }
        Self { levels }
    }
    fn sample(&self, dir: Vec3, lod: f32) -> Vec3 {
        let lod = lod.clamp(0.0, (self.levels.len() - 1) as f32);
        let i = lod.floor() as usize;
        let j = (i + 1).min(self.levels.len() - 1);
        self.levels[i]
            .sample(dir)
            .lerp(self.levels[j].sample(dir), lod - i as f32)
    }
}
fn direction(u: f64, v: f64) -> DVec3 {
    let phi = TAU * (u - 0.5);
    let theta = PI * v;
    DVec3::new(
        phi.cos() * theta.sin(),
        phi.sin() * theta.sin(),
        theta.cos(),
    )
}
fn row_solid_angle(width: usize, height: usize, y: usize) -> f64 {
    TAU / width as f64
        * ((PI * y as f64 / height as f64).cos() - (PI * (y + 1) as f64 / height as f64).cos())
}
fn luminance(v: Vec3) -> f64 {
    v.x as f64 * 0.2126 + v.y as f64 * 0.7152 + v.z as f64 * 0.0722
}
fn basis(d: DVec3) -> [f64; 9] {
    let (x, y, z) = (d.x, d.y, d.z);
    [
        0.28209479177387814,
        0.4886025119029199 * y,
        0.4886025119029199 * z,
        0.4886025119029199 * x,
        1.0925484305920792 * x * y,
        1.0925484305920792 * y * z,
        0.31539156525252005 * (3.0 * z * z - 1.0),
        1.0925484305920792 * x * z,
        0.5462742152960396 * (x * x - y * y),
    ]
}
fn radical_inverse(bits: u32) -> f32 {
    bits.reverse_bits() as f64 as f32 * 2.3283064365386963e-10_f32
}
fn hammersley(i: u32, n: u32) -> Vec2 {
    Vec2::new(i as f32 / n as f32, radical_inverse(i))
}
fn importance_ggx(xi: Vec2, roughness: f32, n: Vec3) -> Vec3 {
    let a = roughness * roughness;
    let phi = std::f32::consts::TAU * xi.x;
    let cos_theta = ((1.0 - xi.y) / (1.0 + (a * a - 1.0) * xi.y)).sqrt();
    let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
    let h = Vec3::new(phi.cos() * sin_theta, phi.sin() * sin_theta, cos_theta);
    let up = if n.z.abs() < 0.999 { Vec3::Z } else { Vec3::X };
    let tangent = up.cross(n).normalize();
    let bitangent = n.cross(tangent);
    (tangent * h.x + bitangent * h.y + n * h.z).normalize()
}
fn extract_sun(base: &mut Level) -> serde_json::Value {
    // A compact bright component in the upper hemisphere is separated from its
    // measured neighboring sky. No synthetic sun color or ambient tint is introduced.
    let mut peak = 0.0;
    let mut peak_index = 0;
    let mut logs = 0.0;
    let mut total = 0.0;
    for y in 0..base.height / 2 {
        let dw = row_solid_angle(base.width, base.height, y);
        for x in 0..base.width {
            let i = y * base.width + x;
            let lum = luminance(base.pixels[i]);
            if lum > peak {
                peak = lum;
                peak_index = i;
            }
            logs += lum.max(1e-6).ln() * dw;
            total += dw;
        }
    }
    let peak_dir = direction(
        (peak_index % base.width) as f64 / base.width as f64 + 0.5 / base.width as f64,
        (peak_index / base.width) as f64 / base.height as f64 + 0.5 / base.height as f64,
    );
    // Robust per-channel median in the 2°–5° annulus estimates sky under the sun.
    let inner = (2.0_f64.to_radians()).cos();
    let outer = (5.0_f64.to_radians()).cos();
    let mut channels: [Vec<f32>; 3] = std::array::from_fn(|_| Vec::new());
    for y in 0..base.height / 2 {
        for x in 0..base.width {
            let d = direction(
                (x as f64 + 0.5) / base.width as f64,
                (y as f64 + 0.5) / base.height as f64,
            );
            let dot = d.dot(peak_dir);
            if dot < inner && dot > outer {
                let p = base.pixels[y * base.width + x];
                channels[0].push(p.x);
                channels[1].push(p.y);
                channels[2].push(p.z);
            }
        }
    }
    let mut background = Vec3::ZERO;
    for c in 0..3 {
        channels[c].sort_unstable_by(f32::total_cmp);
        background[c] = channels[c]
            .get(channels[c].len() / 2)
            .copied()
            .unwrap_or(0.0);
    }
    let threshold = (luminance(background) * 4.0).max(peak * 0.005);
    let cone = (3.0_f64.to_radians()).cos();
    let mut integrated = DVec3::ZERO;
    let mut weighted_direction = DVec3::ZERO;
    let mut solid_angle = 0.0;
    let mut removed_pixels = 0;
    let mut angular_moment = 0.0;
    if peak > luminance(background) * 8.0 {
        for y in 0..base.height / 2 {
            let dw = row_solid_angle(base.width, base.height, y);
            for x in 0..base.width {
                let d = direction(
                    (x as f64 + 0.5) / base.width as f64,
                    (y as f64 + 0.5) / base.height as f64,
                );
                let i = y * base.width + x;
                let p = base.pixels[i];
                let lum = luminance(p);
                if d.dot(peak_dir) > cone && lum > threshold {
                    let excess = (p - background).max(Vec3::ZERO);
                    let weight = luminance(excess) * dw;
                    integrated += excess.as_dvec3() * dw;
                    weighted_direction += d * weight;
                    angular_moment += weight * d.dot(peak_dir).clamp(-1.0, 1.0).acos().powi(2);
                    solid_angle += dw;
                    removed_pixels += 1;
                    base.pixels[i] = p - excess;
                }
            }
        }
    }
    let sun_dir = weighted_direction.try_normalize().unwrap_or(peak_dir);
    let energy = integrated.x * 0.2126 + integrated.y * 0.7152 + integrated.z * 0.0722;
    let rms_angle = if energy > 0.0 {
        (angular_moment / energy).sqrt()
    } else {
        0.0
    };
    eprintln!(
        "Sun peak {peak:.3}, measured sky {:?}, extracted {} pixels / {:.7} sr, integrated {:?}",
        background, removed_pixels, solid_angle, integrated
    );
    json!({"direction":sun_dir.to_array(),"radiance":integrated.to_array(),"solid_angle":solid_angle,"rms_angle_radians":rms_angle,"peak_luminance":peak,"local_sky_radiance":background.to_array(),"removed_pixels":removed_pixels,"threshold_luminance":threshold,"upper_hemisphere_log_average":(logs/total).exp(),"radiance_units":"integrated scene-linear RGB radiance over solid angle; directional irradiance perpendicular to light", "extraction":"upper hemisphere peak; >max(0.005*peak,4*local-sky-luminance), within 3 degrees; positive excess above median 2–5 degree annulus removed"})
}
fn project_sh(base: &Level) -> [[f64; 3]; 9] {
    let mut coefficients = [DVec3::ZERO; 9];
    for y in 0..base.height {
        let dw = row_solid_angle(base.width, base.height, y);
        for x in 0..base.width {
            let d = direction(
                (x as f64 + 0.5) / base.width as f64,
                (y as f64 + 0.5) / base.height as f64,
            );
            let b = basis(d);
            let radiance = base.pixels[y * base.width + x].as_dvec3() * dw;
            for k in 0..9 {
                coefficients[k] += radiance * b[k];
            }
        }
    }
    for (k, c) in coefficients.iter_mut().enumerate() {
        *c *= if k == 0 {
            1.0
        } else if k < 4 {
            2.0 / 3.0
        } else {
            0.25
        };
    }
    coefficients.map(|c| c.to_array())
}
fn prefilter(env: &Environment, width: usize, height: usize, roughness: f32) -> Vec<f32> {
    let mut values = vec![0.0; width * height * 3];
    let base = &env.levels[0];
    let pixel_lod = (base.width as f32 / width as f32).log2().max(0.0);
    for y in 0..height {
        for x in 0..width {
            let n = direction(
                (x as f64 + 0.5) / width as f64,
                (y as f64 + 0.5) / height as f64,
            )
            .as_vec3();
            let color = if roughness <= 0.0 {
                env.sample(n, pixel_lod)
            } else {
                let mut sum = DVec3::ZERO;
                let mut weight = 0.0_f64;
                for i in 0..ENV_SAMPLES {
                    let h = importance_ggx(hammersley(i, ENV_SAMPLES), roughness, n);
                    let vdoth = n.dot(h).max(0.0);
                    let l = (2.0 * vdoth * h - n).normalize();
                    let ndotl = n.dot(l).max(0.0);
                    if ndotl > 0.0 {
                        let a = roughness * roughness;
                        let a2 = a * a;
                        let den = vdoth * vdoth * (a2 - 1.0) + 1.0;
                        let d = a2 / (std::f32::consts::PI * den * den);
                        let pdf = (d * 0.25).max(1e-7);
                        let sample_area = 1.0 / (ENV_SAMPLES as f32 * pdf);
                        let sin_theta = (1.0 - l.z * l.z).max(1e-6).sqrt();
                        let texel_area = std::f32::consts::TAU / base.width as f32
                            * std::f32::consts::PI
                            / base.height as f32
                            * sin_theta;
                        let lod = (0.5 * (sample_area / texel_area).log2()).max(0.0);
                        sum += env.sample(l, lod).as_dvec3() * ndotl as f64;
                        weight += ndotl as f64;
                    }
                }
                (sum / weight.max(1e-12)).as_vec3()
            };
            values[(y * width + x) * 3..(y * width + x) * 3 + 3].copy_from_slice(&color.to_array());
        }
    }
    values
}
fn brdf_lut() -> Vec<f32> {
    let mut values = vec![0.0; LUT_SIZE * LUT_SIZE * 2];
    for y in 0..LUT_SIZE {
        let roughness = (y as f32 + 0.5) / LUT_SIZE as f32;
        let k = roughness * roughness / 2.0;
        for x in 0..LUT_SIZE {
            let ndotv = (x as f32 + 0.5) / LUT_SIZE as f32;
            let v = Vec3::new((1.0 - ndotv * ndotv).sqrt(), 0.0, ndotv);
            let mut a = 0.0;
            let mut b = 0.0;
            for i in 0..LUT_SAMPLES {
                let h = importance_ggx(hammersley(i, LUT_SAMPLES), roughness, Vec3::Z);
                let vdoth = v.dot(h).max(0.0);
                let l = (2.0 * vdoth * h - v).normalize();
                let ndotl = l.z.max(0.0);
                let ndoth = h.z.max(0.0);
                if ndotl > 0.0 {
                    let g_v = ndotv / (ndotv * (1.0 - k) + k);
                    let g_l = ndotl / (ndotl * (1.0 - k) + k);
                    let gvis = g_v * g_l * vdoth / (ndoth * ndotv).max(1e-7);
                    let fc = (1.0 - vdoth).powi(5);
                    a += (1.0 - fc) * gvis;
                    b += fc * gvis;
                }
            }
            values[(y * LUT_SIZE + x) * 2] = a / LUT_SAMPLES as f32;
            values[(y * LUT_SIZE + x) * 2 + 1] = b / LUT_SAMPLES as f32;
        }
    }
    values
}
fn write_floats(path: &Path, values: &[f32]) -> Result<(), Box<dyn Error>> {
    if values.iter().any(|v| !v.is_finite()) {
        return Err(format!("non-finite bake output: {}", path.display()).into());
    }
    let mut writer = BufWriter::new(File::create(path)?);
    for value in values {
        writer.write_all(&value.to_le_bytes())?;
    }
    writer.flush()?;
    if writer.get_ref().metadata()?.len() != (values.len() * 4) as u64 {
        return Err(format!("incorrect packed float dimensions: {}", path.display()).into());
    }
    Ok(())
}
fn main() -> Result<(), Box<dyn Error>> {
    let started = Instant::now();
    let directory = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("assets/apartment"));
    let source = directory.join("balcony.hdr");
    let image = ImageReader::open(&source)?.decode()?.into_rgb32f();
    let (width, height) = image.dimensions();
    if width != height * 2 {
        return Err("expected a 2:1 equirectangular HDR".into());
    }
    let pixels: Vec<Vec3> = image
        .pixels()
        .map(|p| Vec3::from_array(p.0).max(Vec3::ZERO))
        .collect();
    if pixels.iter().any(|p| !p.is_finite()) {
        return Err("HDR contains non-finite radiance".into());
    }
    let mut base = Level {
        width: width as usize,
        height: height as usize,
        pixels,
    };
    let mut logs = 0.0;
    let mut lum_sum = 0.0;
    for y in 0..base.height {
        let dw = row_solid_angle(base.width, base.height, y);
        for x in 0..base.width {
            let l = luminance(base.pixels[y * base.width + x]);
            logs += l.max(1e-6).ln() * dw;
            lum_sum += l * dw;
        }
    }
    let log_average = (logs / (4.0 * PI)).exp();
    let exposure = 0.18 / log_average;
    let sun = extract_sun(&mut base);
    let sh = project_sh(&base);
    if !exposure.is_finite() || sh.iter().flatten().any(|c| !c.is_finite()) {
        return Err("non-finite environment exposure or spherical harmonics".into());
    }
    let env = Environment::new(base);
    let mut packed = Vec::new();
    let mut mip_metadata = Vec::new();
    for level in 0..PREFILTER_LEVELS {
        let width = (PREFILTER_WIDTH >> level).max(1);
        let height = (PREFILTER_HEIGHT >> level).max(1);
        let roughness = level as f32 / (PREFILTER_LEVELS - 1) as f32;
        let offset = packed.len() * 4;
        eprintln!("Prefilter mip {level}: {width}×{height}, roughness {roughness:.3}");
        let pixels = prefilter(&env, width, height, roughness);
        packed.extend(pixels);
        mip_metadata.push(
            json!({"width":width,"height":height,"offset_bytes":offset,"roughness":roughness}),
        );
    }
    write_floats(&directory.join("specular.bin"), &packed)?;
    let lut = brdf_lut();
    write_floats(&directory.join("brdf.bin"), &lut)?;
    let metadata = json!({"version":1,"source":{"filename":"balcony.hdr","width":width,"height":height},"exposure":exposure,"log_average_luminance":log_average,"mean_luminance":lum_sum/(4.0*PI),"exposure_rule":"0.18 / solid-angle-weighted geometric mean HDR luminance; apply once to background and shaded surfaces before shared tone mapping", "coordinate_system":{"mapping":"dir=(cos(phi)*sin(theta),sin(phi)*sin(theta),cos(theta)); phi=TAU*(u-0.5), theta=PI*v","up":"+Z","row_order":"top (v=0,+Z) to bottom (v=1,-Z)","rotation":"none"},"sh9":sh,"sh9_normalization":"normalized real basis [1,y,z,x,xy,yz,3z^2-1,xz,x^2-y^2]; coefficients include Lambert convolution / pi: band factors 1,2/3,1/4; evaluate sum(sh9[i]*Y_i(normal)), then clamp negative truncation ringing to zero","sun":sun,"specular":{"filename":"specular.bin","width":PREFILTER_WIDTH,"height":PREFILTER_HEIGHT,"levels":PREFILTER_LEVELS,"components":3,"format":"f32le","samples":ENV_SAMPLES,"offsets":mip_metadata,"normalization":"sum(Li * NdotL) / sum(NdotL), GGX importance samples, V=N; no exposure; sun excess removed; source mips selected from GGX pdf and equirectangular texel solid angle"},"brdf":{"filename":"brdf.bin","width":LUT_SIZE,"height":LUT_SIZE,"components":2,"format":"f32le","samples":LUT_SAMPLES,"mapping":"texel centers: u=NdotV, v=perceptual roughness; rows increase roughness","normalization":"RG=mean((1-Fc)*Gvis, Fc*Gvis); alpha=roughness^2; Schlick-GGX k=roughness^2/2; Fc=(1-VdotH)^5; use F0*R+G"},"references":["https://learnopengl.com/PBR/IBL/Diffuse-irradiance","https://learnopengl.com/PBR/IBL/Specular-IBL","https://github.com/KhronosGroup/glTF-Sample-Renderer"]});
    fs::write(
        directory.join("lighting.json"),
        serde_json::to_vec_pretty(&metadata)?,
    )?;
    eprintln!(
        "Baked finite {} RGB floats + {} BRDF floats in {:.2}s; exposure {:.6}",
        packed.len(),
        lut.len(),
        started.elapsed().as_secs_f64(),
        exposure
    );
    Ok(())
}
