"""Convert the attributed Appartement GLB into native room triangles.

Run: python3 tools/prepare-room.py [assets/room-source/appartement.glb] [assets/room]
Requires numpy. Texture bytes and glTF UV coordinates are preserved.
"""
import argparse
import json
import struct
from pathlib import Path

import numpy as np


# Coordinates measured from the source world's horizontal dining-table triangles.
# Its clear 2.14578 × 0.806176 m top supports the native deck at original scale.
# Position the player near the living-room end, facing the open length of table.
# The default camera then looks past the player toward the furnished living room.
ROOM_SCALE = 1.0
TABLE_ANCHOR = (-2.95, 0.7543457, 2.129429)
GLTF_TO_NATIVE = np.array([
    [0, 0, -ROOM_SCALE, ROOM_SCALE * TABLE_ANCHOR[2]],
    [-ROOM_SCALE, 0, 0, ROOM_SCALE * TABLE_ANCHOR[0]],
    [0, ROOM_SCALE, 0, -ROOM_SCALE * TABLE_ANCHOR[1] - 0.080],
    [0, 0, 0, 1],
], dtype=float)


def read_glb(path):
    data = Path(path).read_bytes()
    magic, version, length = struct.unpack_from('<III', data)
    assert magic == 0x46546C67 and version == 2 and length == len(data)
    offset, document, binary = 12, None, None
    while offset < length:
        size, kind = struct.unpack_from('<II', data, offset)
        chunk = data[offset + 8:offset + 8 + size]
        if kind == 0x4E4F534A:
            document = json.loads(chunk)
        elif kind == 0x004E4942:
            binary = chunk
        offset += 8 + size
    assert document is not None and binary is not None
    return document, binary


def accessor(document, binary, index):
    value = document['accessors'][index]
    assert 'sparse' not in value, 'Sparse accessors are unsupported'
    view = document['bufferViews'][value['bufferView']]
    assert view.get('buffer', 0) == 0
    dtype = np.dtype({5120: 'i1', 5121: 'u1', 5122: '<i2', 5123: '<u2',
                      5125: '<u4', 5126: '<f4'}[value['componentType']])
    width = {'SCALAR': 1, 'VEC2': 2, 'VEC3': 3, 'VEC4': 4}[value['type']]
    offset = view.get('byteOffset', 0) + value.get('byteOffset', 0)
    result = np.ndarray((value['count'], width), dtype=dtype, buffer=binary,
                        offset=offset, strides=(view.get('byteStride', width * dtype.itemsize), dtype.itemsize)).copy()
    if value.get('normalized'):
        info = np.iinfo(dtype)
        result = np.maximum(result.astype(np.float64) / info.max, -1)
    return result


def node_matrix(node):
    if 'matrix' in node:
        return np.array(node['matrix'], dtype=np.float64).reshape(4, 4, order='F')
    x, y, z, w = node.get('rotation', [0, 0, 0, 1])
    rotation = np.array([
        [1-2*(y*y+z*z), 2*(x*y-z*w), 2*(x*z+y*w)],
        [2*(x*y+z*w), 1-2*(x*x+z*z), 2*(y*z-x*w)],
        [2*(x*z-y*w), 2*(y*z+x*w), 1-2*(x*x+y*y)]])
    result = np.eye(4)
    result[:3, :3] = rotation @ np.diag(node.get('scale', [1, 1, 1]))
    result[:3, 3] = node.get('translation', [0, 0, 0])
    return result


def primitives(document, binary, placement):
    def visit(index, parent):
        node = document['nodes'][index]
        world = parent @ node_matrix(node)
        transform = placement @ world
        if 'mesh' in node:
            for primitive in document['meshes'][node['mesh']]['primitives']:
                assert primitive.get('mode', 4) == 4
                attributes = primitive['attributes']
                position = accessor(document, binary, attributes['POSITION'])
                normal = accessor(document, binary, attributes['NORMAL'])
                uv = accessor(document, binary, attributes['TEXCOORD_0'])
                position = position @ transform[:3, :3].T + transform[:3, 3]
                normal = normal @ np.linalg.inv(transform[:3, :3])
                normal /= np.linalg.norm(normal, axis=1, keepdims=True)
                indices = (accessor(document, binary, primitive['indices']).ravel()
                           if 'indices' in primitive else np.arange(len(position)))
                assert len(indices) % 3 == 0
                if np.linalg.det(transform[:3, :3]) < 0:
                    indices = indices.reshape(-1, 3)[:, [0, 2, 1]].ravel()
                vertices = np.column_stack((position, normal, uv))[indices].astype('<f4')
                assert np.isfinite(vertices).all()
                yield node['mesh'], primitive['material'], vertices
        for child in node.get('children', []):
            yield from visit(child, world)

    scene = document['scenes'][document.get('scene', 0)]
    for root in scene['nodes']:
        yield from visit(root, np.eye(4))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', nargs='?', default='assets/room-source/appartement.glb')
    parser.add_argument('output', nargs='?', default='assets/room')
    args = parser.parse_args()
    document, binary = read_glb(args.source)
    output = Path(args.output)
    output.mkdir(parents=True, exist_ok=True)
    materials = []
    for index, material in enumerate(document['materials']):
        texture = material['pbrMetallicRoughness']['baseColorTexture']
        assert texture.get('texCoord', 0) == 0
        assert 'KHR_texture_transform' not in texture.get('extensions', {})
        source = document['images'][document['textures'][texture['index']]['source']]
        view = document['bufferViews'][source['bufferView']]
        start = view.get('byteOffset', 0)
        extension = {'image/jpeg': 'jpg', 'image/png': 'png'}[source['mimeType']]
        filename = f'texture-{index}.{extension}'
        (output / filename).write_bytes(binary[start:start + view['byteLength']])
        materials.append({'texture': filename, 'double_sided': material.get('doubleSided', False)})

    # Includes glTF Y-up→native Z-up and points the player's front along the table.
    placement = GLTF_TO_NATIVE
    meshes, first = [], 0
    with (output / 'scene.bin').open('wb') as stream:
        for source_mesh, material, vertices in primitives(document, binary, placement):
            vertices.tofile(stream)
            meshes.append({'first': first, 'count': len(vertices), 'material': material})
            print(source_mesh, len(vertices), vertices[:, :3].min(0), vertices[:, :3].max(0))
            first += len(vertices)
    assert first < 2**31 and (output / 'scene.bin').stat().st_size == first * 32
    manifest = {
        'meshes': meshes,
        'materials': materials,
        'placement': {
            'gltf_to_native': placement.tolist(),
            'deck_origin_native': [0, 0, 0],
            'tabletop_native_z': -0.080,
            'table_anchor_gltf': list(TABLE_ANCHOR),
            'room_scale': ROOM_SCALE,
        },
    }
    (output / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(f'{first // 3:,} triangles, {first * 32:,} bytes')


if __name__ == '__main__':
    main()
