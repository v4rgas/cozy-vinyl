`pbr.glsl` provides shared linear HDR lighting for the table, turntable, disc and
photographic background: SH9 diffuse irradiance, GGX-prefiltered specular, a
split-sum BRDF lookup and the measured sun with its shadow map. The Rust loaders
prepend it to the material fragment shaders and select desktop or GLES headers.
There are no studio softboxes, procedural apartment walls or decorative sleeves.

`deck.bin` contains the exact 25 original meshes from `src/render.js`, including
the J-shaped tonearm, counterweight, cartridge, rounded plinth and strobe rim.
It is little-endian, interleaved `f32` position/normal data (24 bytes per vertex).
The offsets and lengths live in `../mesh.rs`. Both the Rust binary and its
package embed these static vertices; no browser or JavaScript runs at runtime.

The baked geometry is maintained as a native asset. The old browser geometry generator is not included.
