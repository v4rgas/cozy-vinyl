# Room rendering

Cozy Vinyl uses [Appartement by SrMonteiro](https://sketchfab.com/3d-models/appartement-6a7a5fe208344b2e8123a88923dbd5b3), a furnished apartment licensed CC BY 4.0. Geometry, attribution and placement details are in [the room asset notes](../assets/room/README.md).

## Perspective

The earlier photographic balcony was an infinite panorama, while the table and player were finite geometry. Orbiting revealed the lack of parallax in the photographed floor and railings. A higher resolution panorama would not fix that mismatch.

The apartment now supplies real walls, floor, furniture and table geometry, all rendered by the same perspective camera as the player. The default vertical field of view is 50 degrees. This is a practical desktop framing choice, not a claim to reproduce the full human visual field. Optical zoom changes field of view; the stylus inspection camera is separate. Orbit limits keep the camera on the table viewing area.

## Materials and lighting

The GLB uses `KHR_materials_unlit`: its textures already contain the artist's lighting and shadows. The room shader samples them as sRGB and preserves their appearance without adding another lighting calculation or exposure curve. See [Khronos's unlit extension](https://github.com/KhronosGroup/glTF/tree/main/extensions/2.0/Khronos/KHR_materials_unlit).

The interactive player remains dynamically shaded. An offline emission-only capture of the apartment at the player supplies its diffuse light and reflections. The probe is converted through the inverse of our ACES approximation so reflection colors can match the visible room after tone mapping. This does not recover missing HDR energy from the original LDR textures.

The Rust environment baker computes SH9 diffuse lighting, GGX roughness levels and a split-sum BRDF lookup. Room probes disable sun extraction: a bright patch in an already-lit texture should not become an additional sun. A modest local shadow from the player grounds it on the table without darkening the entire baked room again.

References: [diffuse irradiance](https://learnopengl.com/PBR/IBL/Diffuse-irradiance), [specular IBL](https://learnopengl.com/PBR/IBL/Specular-IBL), and [Khronos's renderer implementation](https://raw.githubusercontent.com/KhronosGroup/glTF-Sample-Renderer/main/source/Renderer/shaders/ibl.glsl).

## Limits

Lighting in the room is baked and cannot respond fully to moving furniture or changing time of day. The player uses a single local lighting probe and an approximate contact shadow, not real-time global illumination. The room retains its original scale, with the player placed on the dining table. The physical audio model is unchanged.
