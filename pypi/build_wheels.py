#!/usr/bin/env python3
"""Build platform-specific Python wheels containing the pq binary.

Each wheel contains only the native binary and a thin wrapper script.
The wheel platform tags ensure pip installs only the correct one.

Usage:
    python3 pypi/build_wheels.py --version 0.1.0 --binaries-dir dist/
"""

import argparse
import base64
import csv
import hashlib
import io
import os
import stat
import zipfile

# (binary_name, wheel_platform_tag)
PLATFORMS = [
    ("pq-darwin-arm64", "macosx_11_0_arm64"),
    ("pq-linux-amd64", "manylinux_2_17_x86_64.manylinux2014_x86_64"),
    ("pq-linux-arm64", "manylinux_2_17_aarch64.manylinux2014_aarch64"),
]

METADATA_TEMPLATE = """\
Metadata-Version: 2.1
Name: pqtool
Version: {version}
Summary: A Parquet Swiss Army Knife - inspect, query, transform, and view Parquet files
Home-page: https://pqtool.dev
Author: Joe Walnes
License: MIT
Project-URL: Repository, https://github.com/joewalnes/pq
Keywords: parquet,sql,data,cli,viewer
Classifier: Development Status :: 4 - Beta
Classifier: Environment :: Console
Classifier: License :: OSI Approved :: MIT License
Classifier: Topic :: Database
Classifier: Topic :: Utilities
"""

WRAPPER_SCRIPT = """\
#!/usr/bin/env python3
import os, sys
bin_path = os.path.join(os.path.dirname(__file__), "pq")
os.execv(bin_path, [bin_path] + sys.argv[1:])
"""


def sha256_digest(data: bytes) -> str:
    return base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode()


def build_wheel(binary_path: str, platform_tag: str, version: str, out_dir: str) -> str:
    pkg = "pqtool"
    tag = f"py3-none-{platform_tag}"
    wheel_name = f"{pkg}-{version}-{tag}.whl"
    wheel_path = os.path.join(out_dir, wheel_name)
    dist_info = f"{pkg}-{version}.dist-info"
    data_dir = f"{pkg}-{version}.data/scripts"

    records = []

    with zipfile.ZipFile(wheel_path, "w", zipfile.ZIP_DEFLATED) as whl:
        # Add binary
        with open(binary_path, "rb") as f:
            binary_data = f.read()

        bin_info = zipfile.ZipInfo(f"{data_dir}/pq")
        bin_info.external_attr = (stat.S_IRWXU | stat.S_IRGRP | stat.S_IXGRP | stat.S_IROTH | stat.S_IXOTH) << 16
        whl.writestr(bin_info, binary_data)
        records.append((f"{data_dir}/pq", sha256_digest(binary_data), len(binary_data)))

        # METADATA
        metadata = METADATA_TEMPLATE.format(version=version).encode()
        whl.writestr(f"{dist_info}/METADATA", metadata)
        records.append((f"{dist_info}/METADATA", sha256_digest(metadata), len(metadata)))

        # WHEEL
        wheel_meta = f"Wheel-Version: 1.0\nGenerator: pq-build\nRoot-Is-Purelib: false\nTag: {tag}\n".encode()
        whl.writestr(f"{dist_info}/WHEEL", wheel_meta)
        records.append((f"{dist_info}/WHEEL", sha256_digest(wheel_meta), len(wheel_meta)))

        # entry_points.txt
        entry = b"[console_scripts]\npq = pqtool:main\n"
        whl.writestr(f"{dist_info}/entry_points.txt", entry)
        records.append((f"{dist_info}/entry_points.txt", sha256_digest(entry), len(entry)))

        # top_level.txt
        top = b"pqtool\n"
        whl.writestr(f"{dist_info}/top_level.txt", top)
        records.append((f"{dist_info}/top_level.txt", sha256_digest(top), len(top)))

        # RECORD (must be last, no hash for itself)
        buf = io.StringIO()
        writer = csv.writer(buf)
        for path, digest, size in records:
            writer.writerow([path, f"sha256={digest}", size])
        writer.writerow([f"{dist_info}/RECORD", "", ""])
        whl.writestr(f"{dist_info}/RECORD", buf.getvalue())

    return wheel_path


def main():
    parser = argparse.ArgumentParser(description="Build pq Python wheels")
    parser.add_argument("--version", required=True)
    parser.add_argument("--binaries-dir", required=True, help="Directory containing pq-<platform> binaries")
    parser.add_argument("--out-dir", default="pypi/dist", help="Output directory for wheels")
    args = parser.parse_args()

    os.makedirs(args.out_dir, exist_ok=True)

    for binary_name, platform_tag in PLATFORMS:
        binary_path = os.path.join(args.binaries_dir, binary_name)
        if not os.path.exists(binary_path):
            print(f"  skip: {binary_name} (not found)")
            continue
        wheel = build_wheel(binary_path, platform_tag, args.version, args.out_dir)
        print(f"  built: {os.path.basename(wheel)}")


if __name__ == "__main__":
    main()
