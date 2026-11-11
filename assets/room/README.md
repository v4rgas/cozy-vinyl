# Appartement room

“Appartement” by **SrMonteiro (crispimrafael)**.

Source: https://sketchfab.com/3d-models/appartement-6a7a5fe208344b2e8123a88923dbd5b3

Licensed under [Creative Commons Attribution 4.0 International](https://creativecommons.org/licenses/by/4.0/).

The original GLB is preserved in `assets/room-source/appartement.glb`.
`python3 tools/prepare-room.py` extracts the original JPEG bytes and converts the
scene into an unindexed little-endian float32 position/normal/UV triangle stream.
The original baked colors and lighting remain in the textures. glTF hierarchy
transforms, normals, triangle winding, material assignments, and UVs are preserved.

The room is rotated into native Z-up coordinates and translated to put the
record player on the clear wooden dining table at the original room scale.
The measured tabletop is 2.14578 × 0.806176 m, with its top 0.754346 m above
the source floor. The player sits near the living-room end of the table,
facing along its length. The default camera looks past it toward the furnished
living room. The complete player footprint is supported by the original table.

The source-world table anchor is `(-2.95, 0.7543457, 2.129429)`.
`manifest.json` records the full source glTF world-to-native transform. The
native tabletop is at `z = -0.080`, touching the player's feet. The rest of
the dining table is toward native `-Y`, and the living room is toward `+Y`.

These conversions and the placement are modifications by the Cozy Vinyl project.

## Rebuilding

The conversion tools need Python 3 with NumPy and Pillow. Lighting capture also
needs Blender with Cycles and the Rust build dependencies.

```sh
python3 tools/prepare-room.py
python3 tools/bake-room-lighting.py
```

The lighting probe captures only the existing baked emission, with a neutral
blue-grey fill beyond window openings. Its native-coordinate radiance supplies
SH9 diffuse lighting, GGX specular mips and a BRDF lookup. The file `balcony.hdr`
is retained as the baker's input filename; its contents are the apartment probe,
not the previous balcony asset. Exposure is one and sun extraction is disabled.
Baked LDR textures cannot recover clipped HDR light energy.
