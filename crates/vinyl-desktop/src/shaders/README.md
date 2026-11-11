`pbr.glsl` shades the interactive turntable with SH9 diffuse lighting,
GGX-prefiltered reflections and a split-sum BRDF lookup from a room probe.
`room_fs.glsl` preserves the apartment's baked unlit textures, adding only a
modest local shadow from the player onto its supporting table.

`deck.bin` contains the 25 turntable meshes, including the J-shaped tonearm,
counterweight, cartridge, rounded plinth and strobe rim. It is little-endian,
interleaved float32 position/normal data (24 bytes per vertex). The offsets and
lengths live in `../mesh.rs`. The binary embeds these vertices; no JavaScript
runs at runtime. The geometry derives from the MIT-licensed original project,
whose copyright notice is retained in the root LICENSE.

Room geometry and materials are prepared by `tools/prepare-room.py`.
Lighting is prepared by `tools/bake-room-lighting.py` and the Rust environment baker.
