//! Immutable vertex data for the original deck, in metres (position followed by normal).
use eframe::glow::{self, HasContext};
use std::collections::HashMap;

pub struct Meshes {
    buffer: glow::Buffer,
    vaos: HashMap<&'static str, (glow::VertexArray, i32)>,
}

impl Meshes {
    pub unsafe fn new(gl: &glow::Context) -> Result<Self, String> {
        unsafe {
            let buffer = gl.create_buffer()?;
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                include_bytes!("shaders/deck.bin"),
                glow::STATIC_DRAW,
            );
            let mut vaos = HashMap::new();
            for &(name, start, length) in PARTS {
                let vao = gl.create_vertex_array()?;
                gl.bind_vertex_array(Some(vao));
                gl.enable_vertex_attrib_array(0);
                gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, 24, (start * 4) as i32);
                gl.enable_vertex_attrib_array(1);
                gl.vertex_attrib_pointer_f32(1, 3, glow::FLOAT, false, 24, (start * 4 + 12) as i32);
                vaos.insert(name, (vao, (length / 6) as i32));
            }
            gl.bind_vertex_array(None);
            Ok(Self { buffer, vaos })
        }
    }
    pub unsafe fn draw(&self, gl: &glow::Context, name: &str) {
        unsafe {
            let &(vao, count) = &self.vaos[name];
            gl.bind_vertex_array(Some(vao));
            gl.draw_arrays(glow::TRIANGLES, 0, count);
        }
    }
    pub unsafe fn destroy(&self, gl: &glow::Context) {
        unsafe {
            for &(vao, _) in self.vaos.values() {
                gl.delete_vertex_array(vao);
            }
            gl.delete_buffer(self.buffer);
        }
    }
}

const PARTS: &[(&str, usize, usize)] = &[
    // Exact deck geometry baked from src/render.js and src/gl-util.js.
    ("floor", 0, 216),
    ("feet", 216, 23040),
    ("body", 23256, 2520),
    ("plate", 25776, 2520),
    ("platter", 28296, 40320),
    ("mat", 68616, 6912),
    ("spindle", 75528, 3456),
    ("edge", 78984, 4608),
    ("disc", 83592, 387072),
    ("strobe", 470664, 3456),
    ("lens", 474120, 216),
    ("buttons", 474336, 15120),
    ("slot", 489456, 2520),
    ("ledStart", 491976, 864),
    ("led33", 492840, 864),
    ("led45", 493704, 864),
    ("ledPitch", 494568, 864),
    ("tower", 495432, 20736),
    ("towerCap", 516168, 2664),
    ("tube", 518832, 24768),
    ("weight", 543600, 7200),
    ("dial", 550800, 4320),
    ("collar", 555120, 4896),
    ("shell", 560016, 3816),
    ("holder", 563832, 216),
];
