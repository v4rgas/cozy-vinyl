# Apartment environment assets

These assets are bundled for offline native rendering. Both originals are CC0:
https://polyhaven.com/license

## Illovo Beach Balcony

- Author: Greg Zaal
- Source: https://polyhaven.com/a/illovo_beach_balcony
- File: `balcony.hdr`, original 4K floating-point radiance panorama
- Download: https://dl.polyhaven.org/file/ph-assets/HDRIs/hdr/4k/illovo_beach_balcony_4k.hdr
- Original MD5: `159bc12b1f313655bd16e524bd3f1bbd`

The same HDR source supplies the distant background and baked lighting. The player and table are actual geometry; the photographed surroundings have no translational parallax.

## Small Wooden Table 01

- Author: Ulan Cabanilla
- Source: https://polyhaven.com/a/small_wooden_table_01
- `table.jpg`: original 2K diffuse texture
- `table_normal.jpg`: original 2K OpenGL normal map
- `table_arm.jpg`: original 2K packed ambient occlusion, roughness and metallic map
- `table.bin`: 3,410 triangles, interleaved little-endian f32 position, normal and UV

Download sources:

- https://dl.polyhaven.org/file/ph-assets/Models/gltf/2k/small_wooden_table_01/small_wooden_table_01_2k.gltf
- https://dl.polyhaven.org/file/ph-assets/Models/gltf/4k/small_wooden_table_01/small_wooden_table_01.bin
- https://dl.polyhaven.org/file/ph-assets/Models/jpg/2k/small_wooden_table_01/small_wooden_table_01_diff_2k.jpg
- https://dl.polyhaven.org/file/ph-assets/Models/jpg/2k/small_wooden_table_01/small_wooden_table_01_nor_gl_2k.jpg
- https://dl.polyhaven.org/file/ph-assets/Models/jpg/2k/small_wooden_table_01/small_wooden_table_01_arm_2k.jpg

Rebuild the native vertex stream from downloaded glTF and buffer:

```sh
python3 tools/prepare-table.py source.gltf source.bin assets/apartment/table.bin
```

The conversion changes glTF Y-up to the player's Z-up coordinates, scales uniformly by 1.4, centers the table at x=0.042 m and places its top at z=-0.080 m to support the player's feet. Textures retain original UV orientation.

The native environment renderer and shaders live in `crates/vinyl-desktop/src/apartment.rs` and its shader files.

## Precomputed lighting

Rebuild from `balcony.hdr` with:

```sh
cargo run --offline --release -p vinyl-desktop --bin bake_environment
```

- `lighting.json`: SH9 diffuse irradiance divided by pi, extracted sun direction/integrated RGB, shared exposure recommendation and file layout metadata.
- `specular.bin`: little-endian RGB f32 GGX-prefiltered equirectangular mip chain, 512×256 down to 1×1, 256 importance samples per texel.
- `brdf.bin`: 128×128 little-endian RG f32 split-sum BRDF lookup, 256 samples per texel.

The compact sun contribution is removed from the indirect lighting bake and rendered separately as a shadow-casting directional source. It remains visible in the original background. No exposure is baked into the lighting textures. All floating-point output is checked for finite values during generation. This preprocessing is not run at app startup.
