# Native Rust player

```sh
cargo run --release -p vinyl-desktop
```

The two crates separate the physical simulation (`vinyl-core`) from device I/O, the record shelf and the native window (`vinyl-desktop`). The application executes Rust and GPU shaders; it does not embed the old browser app.

Use a current stable Rust toolchain. Linux needs the ALSA development library, a working OpenGL driver, and an X11 or Wayland session. On Debian/Ubuntu the usual build packages are `build-essential pkg-config libasound2-dev libxkbcommon-dev libwayland-dev libx11-dev libxi-dev libxcursor-dev libxrandr-dev libgl1-mesa-dev`. The dependencies support other desktop platforms, but macOS and Windows have not been verified in this environment.

The checkout's `assets/` directory is used by default. For a copied executable, supply its asset root explicitly:

```sh
vinyl-desktop --assets-root /path/to/vinyl
```

## Controls

- Click **Power on** or the turntable to open the output device and start the motor.
- **Space** starts/stops the motor with physical acceleration and coasting.
- **C** lifts/lowers the cue lever; **B** knocks the plinth; **Z** zooms to the stylus.
- Drag the headshell to move the arm. Click a groove to cue there. Drag the disc to hold, reverse or scratch it.
- Drag empty space (or use Shift/right drag) to orbit. Scroll to zoom. **Reset view** restores the opening camera.
- **Controls** exposes 33⅓, 45 and 78 rpm, pitch, volume, stylus shape, tracking force, anti-skate, hum, rumble, flutter and cartridge load capacitance.
- Set surface roughness, dust, scratches, warp and eccentricity, then choose **Press again**. This rebuilds the pressing from its source rather than accumulating defects.
- Select a shelf record or **Cut a file**. Dropping audio files onto the window cuts them in order into one side. Supported decoder formats include FLAC, MP3, WAV, Ogg Vorbis and AAC/M4A; encrypted content is not supported.
- **Show physics** displays measured speed, groove velocity, tracking error, contact forces, mistracking and audio processing load.

The record shelf keeps recording attributions and source links. The two Commons CC0 albums are fetched track by track and cached under `cache/`. Subsequent playback of downloaded tracks works offline. The next track is prepared in the background, with a cueing gap at transitions. The app does not cache an entire album until its tracks have been requested. Network failures are displayed in the status area.

## Apartment setting

The player sits on the dining table in [Appartement by SrMonteiro](../assets/room/README.md), licensed CC BY 4.0. The room and its furniture are actual 3D geometry with baked lighting. The room uses its original scale, and the player rests on the clear tabletop. Geometry and textures are bundled for offline use.

The room retains its baked colors. The turntable uses an environment probe captured inside that room, with precomputed diffuse light and roughness-dependent reflections. A local contact shadow grounds the player on the table. See [rendering notes](photorealistic-rendering.md).

The opening vertical field of view is 50 degrees. Scroll changes the lens field of view without shifting the aim toward the arm. **Z** toggles a separate stylus inspection camera and resets lens zoom. Orbiting is limited to the table viewing area to keep the camera within the room.

## Offline audio export

Export the physical engine's output without a display or audio device:

```sh
cargo run --release -p vinyl-desktop -- --render /tmp/vinyl.wav --record house --seconds 8
```

The exporter spins up the platter, places the needle at the first programme groove, settles it, and writes stereo 16-bit WAV at 48 kHz. It reports RMS, peak, clipped samples and solver resets. Existing WAV paths are overwritten. This is also a way to inspect the simulation independently of device routing.

## Migration boundaries

The port preserves the coupled platter/arm/stylus equations, 2–8× integration, finite stylus tracing, unilateral wall contact, electrical cartridge response, RIAA, mastering, pressing and groove-derived rendering. The deck meshes are baked from the original geometry; shader sources remain GLSL. The label typography and GUI panels are drawn natively, so text rendering and panel layout are not pixel-identical to the browser.

No Spotify login or system capture is implemented. The app currently plays shipped discs, imported files and the curated Commons recordings. [The research note](rust-and-system-audio.md) explains the separate metadata and audio routes, platform capture options and the buffering limits of live playback. Desktop widgets and wallpaper hosting remain future work.

No new automated tests were added in this rewrite, as requested. Compilation and manual smoke verification do not establish numerical or perceptual equivalence across all settings. The native executable uses only the Rust crates and prepared assets.

## Verification in this checkout

On Linux, the workspace compiled successfully with `cargo build --offline -p vinyl-desktop`, and formatting/diff checks passed. The native window was opened in Xvfb, the precompiled Rhapsody disc loaded, and clicking Power on started the device callback, platter and cue automation without logged errors. Screenshots were inspected against the original scene.

Manual WAV renders exercised actual asset deserialization, lossless FLAC reconstruction, pressing and physics: Demo EP for 8 seconds (RMS 0.049156, peak 0.457263) and Rhapsody for 3 seconds (RMS 0.017816, peak 0.250081). Both had zero clipped samples and zero solver resets. These are smoke checks, not an acoustic equivalence assessment. Commons network playback and other operating systems were not exercised.

The furnished apartment was also checked in the native window: default view, zoom in/out, motor start and stylus inspection rendered without logged errors. The room footprint and placement were checked against the source mesh.
