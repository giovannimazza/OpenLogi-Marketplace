"""Regenerate the synthetic .lplug4 fixtures. Run from this directory.

The packages hold no real plugin: the payloads are deterministic filler chosen
to exercise stored entries plus fixed- and dynamic-Huffman deflate blocks.
"""
import random
import zipfile
import zlib

STAMP = (2026, 1, 1, 0, 0, 0)
MANIFEST = """\
# Package type. Must be plugin4 for plugins.
type: plugin4
name: Demo
displayName: Demo
description: Synthetic fixture package
pluginFileName: DemoPlugin.dll
version: 1.0
author: OpenLogi tests
minimumLoupedeckVersion: 6.0
pluginFolderWin: bin
pluginFolderMac: bin
supportedDevices:
    - LoupedeckCtFamily
"""


def add(archive, name, data, method=zipfile.ZIP_DEFLATED):
    info = zipfile.ZipInfo(name, STAMP)
    info.compress_type = method
    archive.writestr(info, data)


def build(path, extra=(), manifest=True):
    with zipfile.ZipFile(path, "w") as archive:
        if manifest:
            add(archive, "metadata/LoupedeckPackage.yaml", MANIFEST)
        for name, data, method in extra:
            add(archive, name, data, method)


rng = random.Random(4)
icon = bytes(rng.randrange(256) for _ in range(4096))
words = [b"alpha", b"bravo", b"charlie", b"delta", b"echo", b"foxtrot"]
dll = b" ".join(rng.choice(words) for _ in range(40000))[:200000].ljust(200000, b".")

build("demo.lplug4", [
    ("metadata/Icon256x256.png", icon, zipfile.ZIP_STORED),
    ("bin/", b"", zipfile.ZIP_STORED),
    ("bin/DemoPlugin.dll", dll, zipfile.ZIP_DEFLATED),
    ("bin/notes.txt", b"hello hello hello\n", zipfile.ZIP_DEFLATED),
    ("bin/empty.bin", b"", zipfile.ZIP_STORED),
])
build("zip-slip.lplug4", [("../escaped.txt", b"x", zipfile.ZIP_STORED)])
build("backslash-slip.lplug4", [("..\\escaped.txt", b"x", zipfile.ZIP_STORED)])
build("no-manifest.lplug4", [("bin/notes.txt", b"x", zipfile.ZIP_STORED)], manifest=False)

print(f"icon crc 0x{zlib.crc32(icon):08X}  dll crc 0x{zlib.crc32(dll):08X}")
