"""Read the glTF 2.0 containers, local resources, and accessors used by creature assets."""

import base64
import json
import math
from pathlib import Path, PureWindowsPath
import struct
from urllib.parse import unquote, urlsplit
import zlib


class ValidationError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise ValidationError(message)


def unique_object(pairs):
    value = {}
    for key, item in pairs:
        require(key not in value, f"Duplicate JSON field {key!r}")
        value[key] = item
    return value


def invalid_constant(token):
    raise ValidationError(f"Invalid JSON number {token}")


def read_json(data, label):
    try:
        result = json.loads(data.decode("utf-8"), object_pairs_hook=unique_object, parse_constant=invalid_constant)
    except (UnicodeError, ValueError) as error:
        raise ValidationError(f"{label}: {error}") from error
    return object_value(result, label)


def object_value(value, label):
    require(isinstance(value, dict), f"{label} must be an object")
    return value


def array(value, label, minimum=0):
    require(isinstance(value, list) and len(value) >= minimum, f"{label} must be an array of at least {minimum} entries")
    return value


def integer(value, label, minimum=0):
    require(type(value) is int and value >= minimum, f"{label} must be an integer >= {minimum}")
    return value


def number(value, label):
    # glTF and the Bevy manifest use single-precision floats.
    require(type(value) in (float, int) and -3.4028234663852886e38 <= value <= 3.4028234663852886e38,
            f"{label} must be a finite 32-bit float")
    return value


def vector(value, length, label):
    require(isinstance(value, list) and len(value) == length, f"{label} must have {length} components")
    return [number(item, label) for item in value]


def index_of(sequence, value, label):
    index = integer(value, label)
    require(index < len(sequence), f"{label} index {index} is out of range")
    return index


def read_glb(path):
    data = path.read_bytes()
    require(len(data) >= 20 and struct.unpack_from("<4sII", data) == (b"glTF", 2, len(data)), "Invalid GLB header")
    offset = 12
    first_chunk_end = None
    document = binary = None
    while offset < len(data):
        require(offset + 8 <= len(data), "Truncated GLB chunk header")
        length, kind = struct.unpack_from("<I4s", data, offset)
        start = offset + 8
        end = start + length
        require(length % 4 == 0 and end <= len(data), "Invalid GLB chunk length")
        if offset == 12:
            require(kind == b"JSON", "First GLB chunk must contain JSON")
            document = read_json(data[start:end], "GLB JSON")
            first_chunk_end = end
        elif kind == b"BIN\0":
            require(binary is None and offset == first_chunk_end, "Invalid GLB binary chunk placement")
            binary = data[start:end]
        else:
            require(kind != b"JSON", "Duplicate GLB JSON chunk")
        offset = end
    return document, binary


def read_local_resource(uri, directory, root):
    require(isinstance(uri, str) and uri, "Resource URI must be a nonempty string")
    decoded = unquote(uri, errors="strict")
    if decoded.startswith("data:"):
        header, separator, body = decoded[5:].partition(",")
        mime, _, encoding = header.partition(";")
        require(separator and encoding == "base64", "Resource data URI must use base64")
        return base64.b64decode(body, validate=True), mime
    parts = urlsplit(uri)
    relative = unquote(parts.path, errors="strict")
    require(
        not (parts.scheme or parts.netloc or parts.query or parts.fragment)
        and relative and not Path(relative).is_absolute() and not PureWindowsPath(relative).drive
        and not any(character in relative for character in ("\\", "\0", ":", "#", "?"))
        and not any(character in uri for character in ("\n", "\r", "\t")),
        f"Expected a local relative resource URI: {uri!r}",
    )
    path = (directory / relative).resolve()
    require(path.is_relative_to(root), f"Resource URI escapes the asset root: {uri!r}")
    return path.read_bytes(), None


COMPONENTS = {5120: "b", 5121: "B", 5122: "h", 5123: "H", 5125: "I", 5126: "f"}
SHAPES = {"SCALAR": (1, 1), "VEC2": (1, 2), "VEC3": (1, 3), "VEC4": (1, 4),
          "MAT2": (2, 2), "MAT3": (3, 3), "MAT4": (4, 4)}
NORMALIZERS = {5120: 127, 5121: 255, 5122: 32767, 5123: 65535}
ARRAYS = (
    "accessors", "animations", "buffers", "bufferViews", "cameras", "images", "materials", "meshes",
    "nodes", "samplers", "scenes", "skins", "textures",
)


class Gltf:
    def __init__(self, path, root):
        self.path = path
        self.root = root
        if path.suffix == ".glb":
            self.document, binary = read_glb(path)
        else:
            self.document, binary = read_json(path.read_bytes(), "glTF JSON"), None
        asset = object_value(self.document.get("asset"), "glTF asset")
        require(asset.get("version") == "2.0" and asset.get("minVersion", "2.0") == "2.0", "Expected glTF 2.0")
        self.tables = {}
        for key in ARRAYS:
            entries = array(self.document.get(key, []), f"glTF {key}", int(key in self.document))
            for index, entry in enumerate(entries):
                object_value(entry, f"{key}[{index}]")
                require("name" not in entry or isinstance(entry["name"], str), f"{key}[{index}].name must be a string")
            self.tables[key] = entries
        self.buffers = []
        for index, entry in enumerate(self.tables["buffers"]):
            label = f"buffer[{index}]"
            size = integer(entry.get("byteLength"), f"{label}.byteLength", 1)
            if "uri" in entry:
                data, mime = self.resource(entry["uri"])
                require(mime in (None, "application/octet-stream", "application/gltf-buffer"), f"Invalid MIME type for {label}")
                require(len(data) >= size, f"{label} is longer than its external resource")
            else:
                require(index == 0 and binary is not None, f"{label} has no binary data or URI")
                data = binary
                require(size <= len(data) <= size + 3 and not any(data[size:]), f"Invalid GLB {label} length or padding")
            self.buffers.append(memoryview(data)[:size])
        require(binary is None or (self.tables["buffers"] and "uri" not in self.tables["buffers"][0]),
                "Unreferenced GLB binary chunk")
        self.views = []
        for index, entry in enumerate(self.tables["bufferViews"]):
            label = f"bufferView[{index}]"
            buffer = self.buffers[index_of(self.buffers, entry.get("buffer"), f"{label}.buffer")]
            offset = integer(entry.get("byteOffset", 0), f"{label}.byteOffset")
            size = integer(entry.get("byteLength"), f"{label}.byteLength", 1)
            stride = entry.get("byteStride")
            if stride is not None:
                require(integer(stride, f"{label}.byteStride", 4) <= 252 and stride % 4 == 0, f"Invalid {label}.byteStride")
            require(entry.get("target") in (None, 34962, 34963), f"Invalid {label}.target")
            require(offset + size <= len(buffer), f"{label} overruns its buffer")
            self.views.append((buffer[offset:offset + size], offset, stride))
        self.accessors = {}
        self.total_values = 0

    def resource(self, uri):
        return read_local_resource(uri, self.path.parent, self.root)

    def ref(self, table, value, label):
        return index_of(self.tables[table], value, label)

    def values(self, index, label, shape=None, components=None, normalized=None, allow_sparse=True):
        index = self.ref("accessors", index, label)
        entry = self.tables["accessors"][index]
        require(shape is None or entry.get("type") in shape, f"{label} has the wrong accessor shape")
        require(components is None or entry.get("componentType") in components, f"{label} has the wrong component type")
        require(normalized is None or entry.get("normalized", False) is normalized, f"{label} has invalid normalization")
        require(allow_sparse or "sparse" not in entry, f"{label}: Bevy does not support sparse animation timestamps")
        if index not in self.accessors:
            self.accessors[index] = self.read_accessor(entry, f"accessor[{index}] ({label})")
        return self.accessors[index]

    def rows(self, view_index, offset, count, component, shape, label, packed=False):
        data, view_offset, stride = self.views[index_of(self.views, view_index, label)]
        columns, rows = SHAPES[shape]
        component_size = struct.calcsize(COMPONENTS[component])
        column_size = rows * component_size
        column_stride = column_size + (-column_size % 4 if columns > 1 else 0)
        element_stride = column_stride * columns
        element_size = (columns - 1) * column_stride + column_size
        require(not packed or stride is None, f"{label} sparse data must be packed")
        stride = stride or element_stride
        alignment = 4 if columns > 1 else component_size
        require(offset % component_size == 0 and (view_offset + offset) % alignment == 0, f"{label} is misaligned")
        require(stride >= element_stride and offset + stride * (count - 1) + element_size <= len(data),
                f"{label} overruns its bufferView")
        decode = struct.Struct("<" + COMPONENTS[component] * rows).unpack_from
        starts = range(offset, offset + count * stride, stride)
        if column_stride == column_size:
            decode = struct.Struct("<" + COMPONENTS[component] * (rows * columns)).unpack_from
            return [decode(data, start) for start in starts]
        return [tuple(value for column in range(columns) for value in decode(data, start + column * column_stride))
                for start in starts]

    def read_accessor(self, entry, label):
        component = integer(entry.get("componentType"), f"{label}.componentType")
        shape = entry.get("type")
        require(component in COMPONENTS and isinstance(shape, str) and shape in SHAPES, f"Invalid {label} type")
        count = integer(entry.get("count"), f"{label}.count", 1)
        offset = integer(entry.get("byteOffset", 0), f"{label}.byteOffset")
        normalized = entry.get("normalized", False)
        require(type(normalized) is bool and (not normalized or component in NORMALIZERS), f"Invalid {label}.normalized")
        columns, rows = SHAPES[shape]
        width = columns * rows
        self.total_values += count * width
        require(self.total_values <= 8_000_000, "Decoded accessor data exceeds the static validator capacity")
        if "bufferView" in entry:
            values = self.rows(entry["bufferView"], offset, count, component, shape, label)
        else:
            require("byteOffset" not in entry, f"{label}.byteOffset requires a bufferView")
            values = [(0,) * width] * count
        if "sparse" in entry:
            sparse = object_value(entry["sparse"], f"{label}.sparse")
            sparse_count = integer(sparse.get("count"), f"{label}.sparse.count", 1)
            require(sparse_count <= count, f"{label} has more sparse values than elements")
            keys = object_value(sparse.get("indices"), f"{label}.sparse.indices")
            kind = keys.get("componentType")
            require(kind in (5121, 5123, 5125), f"Invalid {label} sparse index type")
            index_rows = self.rows(keys.get("bufferView"), integer(keys.get("byteOffset", 0), label), sparse_count,
                                   kind, "SCALAR", label, packed=True)
            replacements = object_value(sparse.get("values"), f"{label}.sparse.values")
            items = self.rows(replacements.get("bufferView"), integer(replacements.get("byteOffset", 0), label),
                              sparse_count, component, shape, label, packed=True)
            previous = -1
            for (index,), item in zip(index_rows, items):
                require(previous < index < count, f"{label} sparse indices must increase and fit in the accessor")
                values[index] = item
                previous = index
        require(all(math.isfinite(value) for row in values for value in row), f"Non-finite {label} values")
        for name, extremum in (("min", min), ("max", max)):
            if name in entry:
                bounds = vector(entry[name], width, f"{label}.{name}")
                require(all(math.isclose(expected, extremum(actual), rel_tol=2e-6, abs_tol=1e-7)
                            for expected, actual in zip(bounds, zip(*values))), f"{label}.{name} disagrees with data")
        if normalized:
            maximum = NORMALIZERS[component]
            return [tuple(max(-1, value / maximum) for value in row) for row in values]
        return values

    def validate_remaining_accessors(self):
        for index in range(len(self.tables["accessors"])):
            self.values(index, f"accessor[{index}]")


def png_dimensions(data, label):
    require(data[:8] == b"\x89PNG\r\n\x1a\n", f"{label}: PNG texture required by this Bevy build")
    require(len(data) >= 33 and data[8:16] == b"\0\0\0\rIHDR", f"{label}: invalid PNG header")
    width, height, depth, color, compression, filtering, interlace = struct.unpack_from(">IIBBBBB", data, 16)
    depths = {0: (1, 2, 4, 8, 16), 2: (8, 16), 3: (1, 2, 4, 8), 4: (8, 16), 6: (8, 16)}
    require(0 < width < 2**31 and 0 < height < 2**31 and depth in depths.get(color, ())
            and compression == filtering == 0 and interlace in (0, 1), f"{label}: invalid PNG metadata")
    offset = 8
    has_pixels = False
    while offset + 12 <= len(data):
        size, kind = struct.unpack_from(">I4s", data, offset)
        end = offset + 8 + size
        require(end + 4 <= len(data), f"{label}: truncated PNG chunk")
        require(zlib.crc32(data[offset + 4:end]) == struct.unpack_from(">I", data, end)[0], f"{label}: corrupt PNG chunk")
        has_pixels |= kind == b"IDAT" and size > 0
        if kind == b"IEND":
            require(size == 0 and has_pixels and end + 4 == len(data), f"{label}: invalid PNG end or missing pixel data")
            return width, height
        offset = end + 4
    raise ValidationError(f"{label}: missing PNG end")
