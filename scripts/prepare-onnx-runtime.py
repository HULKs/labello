#!/usr/bin/env python3
"""Prepare the pinned, self-hosted ONNX Runtime Web distribution for Trunk."""
import hashlib
import io
import os
from pathlib import Path
import tarfile
import urllib.request

VERSION = "1.27.0"
SHA256 = "b59c9819434a7519f334f77e8d4bf22b69808d531a57724cabc4bb2c0704c835"
FILES = [
    "ort.webgpu.min.js",
    "ort-wasm-simd-threaded.asyncify.mjs",
    "ort-wasm-simd-threaded.asyncify.wasm",
    "ort-wasm-simd-threaded.jspi.mjs",
    "ort-wasm-simd-threaded.jspi.wasm",
]
ROOT = Path(__file__).resolve().parents[1]
CACHE = ROOT / "target" / "onnx-runtime-package.tgz"
DESTINATION = ROOT / "target" / "onnx-runtime"


def main():
    CACHE.parent.mkdir(parents=True, exist_ok=True)
    archive = CACHE.read_bytes() if CACHE.exists() else None
    if archive is None or hashlib.sha256(archive).hexdigest() != SHA256:
        url = f"https://registry.npmjs.org/onnxruntime-web/-/onnxruntime-web-{VERSION}.tgz"
        with urllib.request.urlopen(url, timeout=60) as response:
            archive = response.read(64 * 1024 * 1024 + 1)
        if len(archive) > 64 * 1024 * 1024 or hashlib.sha256(archive).hexdigest() != SHA256:
            raise SystemExit("ONNX Runtime archive checksum or size mismatch")
        temporary = CACHE.with_suffix(".tmp")
        temporary.write_bytes(archive)
        os.replace(temporary, CACHE)
    DESTINATION.mkdir(parents=True, exist_ok=True)
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as package:
        for name in FILES:
            member = package.getmember(f"package/dist/{name}")
            if not member.isfile() or member.size > 64 * 1024 * 1024:
                raise SystemExit("Invalid ONNX Runtime archive member")
            source = package.extractfile(member)
            if source is None:
                raise SystemExit("Missing ONNX Runtime archive member")
            content = source.read()
            target = DESTINATION / name
            if not target.exists() or target.read_bytes() != content:
                temporary = target.with_suffix(target.suffix + ".tmp")
                temporary.write_bytes(content)
                os.replace(temporary, target)
    (DESTINATION / "LICENSE.txt").write_text(
        "ONNX Runtime Web 1.27.0\nCopyright (c) Microsoft Corporation. All rights reserved.\n\n"
        "MIT License\n\n"
        "Permission is hereby granted, free of charge, to any person obtaining a copy "
        "of this software and associated documentation files (the Software), to deal "
        "in the Software without restriction, including without limitation the rights "
        "to use, copy, modify, merge, publish, distribute, sublicense, and/or sell "
        "copies of the Software, and to permit persons to whom the Software is "
        "furnished to do so, subject to the following conditions:\n\n"
        "The above copyright notice and this permission notice shall be included in "
        "all copies or substantial portions of the Software.\n\n"
        "THE SOFTWARE IS PROVIDED AS IS, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR "
        "IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, "
        "FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE "
        "AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER "
        "LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING "
        "FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS "
        "IN THE SOFTWARE.\n"
    )


if __name__ == "__main__":
    main()
