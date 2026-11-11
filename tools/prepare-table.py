"""Bake the Poly Haven glTF table to the native position/normal/UV triangle stream.
Usage: python3 tools/prepare-table.py source.gltf source.bin assets/apartment/table.bin
The downloaded glTF has one mesh and an identity node transform; reject other layouts.
"""
import json
import math
import struct
import sys
from pathlib import Path

source = json.loads(Path(sys.argv[1]).read_text())
buffer = Path(sys.argv[2]).read_bytes()
assert len(source['nodes']) == 1 and set(source['nodes'][0]) <= {'mesh', 'name'}

def accessor(index):
    a = source['accessors'][index]
    view = source['bufferViews'][a['bufferView']]
    components = {'VEC3': 3, 'VEC2': 2, 'SCALAR': 1}[a['type']]
    kind = {5126: 'f', 5123: 'H', 5125: 'I'}[a['componentType']]
    fmt = '<' + kind * components
    stride = view.get('byteStride', struct.calcsize(fmt))
    offset = view.get('byteOffset', 0) + a.get('byteOffset', 0)
    return [struct.unpack_from(fmt, buffer, offset + i * stride) for i in range(a['count'])]

primitive = source['meshes'][0]['primitives'][0]
assert primitive.get('mode', 4) == 4
positions = accessor(primitive['attributes']['POSITION'])
normals = accessor(primitive['attributes']['NORMAL'])
uvs = accessor(primitive['attributes']['TEXCOORD_0'])
indices = accessor(primitive['indices'])
scale = 1.4
top = max(p[1] for p in positions)
with Path(sys.argv[3]).open('wb') as out:
    for (i,) in indices:
        x, y, z = positions[i]
        nx, ny, nz = normals[i]
        values = (x * scale + 0.042, -z * scale, (y - top) * scale - 0.080,
                  nx, -nz, ny, *uvs[i])
        assert all(math.isfinite(v) for v in values)
        out.write(struct.pack('<8f', *values))
print(f'{len(indices) // 3} triangles; tabletop z=-0.080 m; uniform scale {scale}')
