# Photorealistic rendering approach

Research and implementation direction, 11 November 2026. The aim is a simple, convincing player on a wooden table in the selected Illovo Beach Balcony setting, while preserving native interaction and the audio simulation.

## Diagnosis

The first apartment scene combined three incompatible lighting models: a display-ready JPEG background, approximate table lighting, and the turntable's original studio softboxes. The handmade lamp, striped procedural wood, flat room surfaces and decorative record sleeves amplified that mismatch. Adding higher-resolution meshes alone would not solve it.

## References and useful techniques

- [Khronos glTF PBR guide](https://www.khronos.org/gltf/pbr): use base color, metallic/roughness, normal and occlusion textures consistently. Base color is color data; normal and packed material channels are linear data. The existing scanned table already has these maps, so importing them is more useful than adding polygons.
- [Diffuse irradiance](https://learnopengl.com/PBR/IBL/Diffuse-irradiance): integrate the environment over the surface hemisphere for indirect diffuse light rather than adding a constant ambient tint.
- [Specular IBL](https://learnopengl.com/PBR/IBL/Specular-IBL): use GGX-prefiltered environment levels and a split-sum BRDF lookup table for roughness-dependent reflections. Ordinary JPEG mipmaps are not a replacement for this filtering.
- [Khronos sample renderer's actual IBL implementation](https://raw.githubusercontent.com/KhronosGroup/glTF-Sample-Renderer/main/source/Renderer/shaders/ibl.glsl): diffuse environment, roughness-dependent specular environment and BRDF lookup share one environment rotation/intensity. This was read as prior art for the runtime organization.
- [Selected HDR source](https://polyhaven.com/a/illovo_beach_balcony): morning sun with strong contrast. Preserve that character, and align the shadow-casting light with the sun in this panorama.

## Decision

Use the selected photographic surroundings, the scanned wooden table and the original interactive turntable. Remove the generated apartment walls, lamp, floor and extra record sleeves. The panorama supplies the architectural context; nearby table and player remain actual geometry.

Use the floating-point HDR source for both the background and lighting. Bake diffuse spherical harmonics, GGX specular levels and the BRDF lookup offline. Extract the concentrated sun contribution into a directional light so it can cast shadows; remove that contribution from indirect lighting to avoid counting it twice. Apply one exposure and tone-mapping curve across the backdrop, table and player. Preserve real table detail with its normal and packed AO/roughness/metallic textures. Contact occlusion should attenuate indirect light modestly rather than painting a black rectangle under the player.

This keeps runtime work to ordinary rasterization, a shadow map, and precomputed texture lookups. Ray tracing, screen-space reflections, depth-of-field blur, bloom and additional decorative assets are not needed to address the observed problems. They would add cost and tuning before the basic lighting is consistent.

## Limits

A panorama has angular detail but no translational parallax. The renderer therefore supports close orbiting around the player rather than walking through the photographed balcony. The environment is not a reconstruction of the photographed building. The table is a separately scanned object placed into that view. Shared HDR illumination improves consistency, but it is not full global illumination or an offline path-traced render.
