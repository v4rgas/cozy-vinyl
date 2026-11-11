"""Capture the baked apartment at the deck and build native PBR lighting.

Run from the repository root after tools/prepare-room.py:
    python3 tools/bake-room-lighting.py

Requires Blender (Cycles CPU) and the offline Cargo dependencies. The GLB's
already-lit unlit textures become emission. Exterior openings receive the same
neutral fill as the native renderer; room surfaces receive no additional light.
The panorama uses the same placement matrix as the native triangles.
"""

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

EXTERIOR_DISPLAY_SRGB = (.65, .72, .78)


def capture(args):
    import bpy
    import numpy as np
    from mathutils import Matrix

    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=str(Path(args.source).resolve()))
    manifest = json.loads((Path(args.output) / 'manifest.json').read_text())
    placement = Matrix(manifest['placement']['gltf_to_native'])
    # Blender's importer has already changed glTF Y-up into (x,-z,y).
    blender_to_gltf = Matrix(((1, 0, 0, 0), (0, 0, 1, 0),
                              (0, -1, 0, 0), (0, 0, 0, 1)))
    transform = placement @ blender_to_gltf
    meshes = [obj for obj in bpy.context.scene.objects if obj.type == 'MESH']
    for obj in meshes:
        obj.matrix_world = transform @ obj.matrix_world
    for obj in list(bpy.context.scene.objects):
        if obj.type in {'LIGHT', 'CAMERA'}:
            bpy.data.objects.remove(obj, do_unlink=True)
    # Import preserves UVs, sRGB texture decoding, and unlit material colors.
    # Connect the emission directly, avoiding the importer's camera-ray switch.
    for material in {slot.material for obj in meshes for slot in obj.material_slots}:
        nodes = material.node_tree.nodes
        emission = next(node for node in nodes if node.type == 'EMISSION')
        output = next(node for node in nodes if node.type == 'OUTPUT_MATERIAL')
        surface = emission.outputs['Emission']
        if material.use_backface_culling:
            geometry = nodes.new('ShaderNodeNewGeometry')
            transparent = nodes.new('ShaderNodeBsdfTransparent')
            mix = nodes.new('ShaderNodeMixShader')
            material.node_tree.links.new(geometry.outputs['Backfacing'], mix.inputs[0])
            material.node_tree.links.new(surface, mix.inputs[1])
            material.node_tree.links.new(transparent.outputs[0], mix.inputs[2])
            surface = mix.outputs[0]
        material.node_tree.links.new(surface, output.inputs['Surface'])

    scene = bpy.context.scene
    scene.world = bpy.data.worlds.new('Neutral exterior opening fill')
    scene.world.use_nodes = True
    exterior_linear = tuple(value / 12.92 if value <= .04045 else
                            ((value + .055) / 1.055) ** 2.4
                            for value in EXTERIOR_DISPLAY_SRGB)
    scene.world.node_tree.nodes['Background'].inputs['Color'].default_value = (*exterior_linear, 1)
    scene.render.engine = 'CYCLES'
    scene.cycles.device = 'CPU'
    scene.cycles.samples = args.samples
    scene.cycles.seed = 0
    scene.cycles.use_denoising = False
    scene.cycles.max_bounces = 1
    scene.render.resolution_x = args.width
    scene.render.resolution_y = args.width // 2
    scene.render.resolution_percentage = 100
    scene.render.film_transparent = False
    scene.view_settings.view_transform = 'Standard'
    scene.view_settings.look = 'None'
    scene.view_settings.exposure = 0
    scene.view_settings.gamma = 1
    camera = bpy.data.cameras.new('Deck environment probe')
    camera.type = 'PANO'
    camera.panorama_type = 'EQUIRECTANGULAR'
    probe = bpy.data.objects.new(camera.name, camera)
    scene.collection.objects.link(probe)
    scene.camera = probe
    origin = manifest['placement']['deck_origin_native']
    # Undo Cycles' panorama camera basis. It then has +Z up, +X at u=.5.
    # Cycles increases longitude toward -Y; the horizontal flip below makes it
    # agree with native phi=TAU*(u-.5). Verified with six colored axial walls.
    # https://github.com/blender/blender/blob/main/intern/cycles/blender/camera.cpp
    # https://github.com/blender/blender/blob/main/intern/cycles/kernel/camera/projection.h
    probe.matrix_world = Matrix(((0, 0, -1, origin[0]),
                                 (-1, 0, 0, origin[1]),
                                 (0, 1, 0, origin[2] + args.height),
                                 (0, 0, 0, 1)))
    camera.clip_start = .001
    camera.clip_end = 100
    # EXR avoids display transforms and exposes the scene-linear render pixels.
    scene.render.image_settings.file_format = 'OPEN_EXR'
    scene.render.image_settings.color_depth = '32'
    scene.render.filepath = args.intermediate
    bpy.ops.render.render(write_still=True)
    image = bpy.data.images.load(args.intermediate)
    pixels = np.empty(args.width * (args.width // 2) * 4, dtype=np.float32)
    image.pixels.foreach_get(pixels)
    pixels = pixels.reshape(args.width // 2, args.width, 4)[:, ::-1].copy()
    # Recover a display-matched radiance proxy from the baked LDR appearance.
    # Runtime tone() uses pow(ACES(x),1/2.2), so its inverse target is display
    # sRGB ^ 2.2, rather than the exact sRGB texture-decoding transfer function.
    linear = np.maximum(pixels[:, :, :3].astype(np.float64), 0)
    display = np.where(linear <= .0031308, linear * 12.92,
                       1.055 * np.power(linear, 1 / 2.4) - .055)
    target = np.minimum(np.power(display, 2.2), .999)
    # Positive root of (2.43*y-2.51)x² + (.59*y-.03)x + .14*y = 0.
    a = 2.51 - 2.43 * target
    b = .03 - .59 * target
    pixels[:, :, :3] = ((-b + np.sqrt(b*b + .56*a*target)) / (2*a)).astype(np.float32)
    assert np.isfinite(pixels).all() and (pixels[:, :, :3] >= 0).all()
    result = bpy.data.images.new('Native room environment', width=args.width,
                                 height=args.width // 2, float_buffer=True)
    result.pixels.foreach_set(pixels.ravel())
    result.filepath_raw = str((Path(args.output) / 'balcony.hdr').resolve())
    result.file_format = 'HDR'
    result.save()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', default='assets/room-source/appartement.glb')
    parser.add_argument('--output', default='assets/room')
    parser.add_argument('--blender', default='blender')
    parser.add_argument('--width', type=int, default=1024)
    parser.add_argument('--height', type=float, default=.15, help='probe height above deck, meters')
    parser.add_argument('--samples', type=int, default=16)
    parser.add_argument('--threads', type=int, default=4)
    parser.add_argument('--render', action='store_true', help=argparse.SUPPRESS)
    parser.add_argument('--intermediate', help=argparse.SUPPRESS)
    argv = sys.argv[sys.argv.index('--') + 1:] if '--' in sys.argv else sys.argv[1:]
    args = parser.parse_args(argv)
    if args.width < 64 or args.width % 2:
        parser.error('--width must be an even integer of at least 64')
    if args.render:
        capture(args)
        return
    with tempfile.TemporaryDirectory(prefix='vinyl-room-probe-') as temporary:
        subprocess.run([args.blender, '--background', '--factory-startup', '-noaudio',
                        '--threads', str(args.threads), '--python', str(Path(__file__).resolve()),
                        '--', '--render', '--source', str(Path(args.source).resolve()),
                        '--output', str(Path(args.output).resolve()), '--width', str(args.width),
                        '--height', str(args.height), '--samples', str(args.samples),
                        '--intermediate', str(Path(temporary) / 'room.exr')],
                       env={**os.environ, 'ALSOFT_DRIVERS': 'null'}, check=True)
    subprocess.run(['cargo', 'run', '--release', '--offline', '-p', 'vinyl-desktop',
                    '--bin', 'bake_environment', '--', args.output, '--no-sun'], check=True)
    path = Path(args.output) / 'lighting.json'
    lighting = json.loads(path.read_text())
    lighting['exposure'] = 1.0
    lighting['exposure_rule'] = 'Fixed unity: source LDR appearance was inverse tone mapped for the shared runtime ACES display transform.'
    manifest = json.loads((Path(args.output) / 'manifest.json').read_text())
    position = list(manifest['placement']['deck_origin_native'])
    position[2] += args.height
    lighting['room_probe'] = {
        'source': args.source, 'position_native': position,
        'material_capture': 'Original baked KHR_materials_unlit colors as emission; no added lights.',
        'radiometry': 'Display-matched LDR radiance proxy, not measured HDR illumination; clipped highlights cannot be recovered.',
        'inverse_tone_mapping': 'sRGB-encode source linear RGB, raise to 2.2, inverse ACES fitted curve; target capped .999.',
        'samples': args.samples, 'environment_rotation': 0,
        'sun': 'No extracted directional sun; all captured room radiance remains in diffuse and specular IBL.',
        'exterior_fill_display_srgb': EXTERIOR_DISPLAY_SRGB,
        'exterior_fill': 'Neutral artist-selected background for geometry openings; matches native clear color. Source is sRGB-decoded before the shared inverse tone mapping.',
    }
    path.write_text(json.dumps(lighting, indent=2) + '\n')
    print(f'Room probe and PBR lighting written to {args.output}; exposure 1, rotation 0.')


if __name__ == '__main__':
    main()
