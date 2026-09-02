#!/usr/bin/env python3
"""Build platform-specific Python wheels containing the pq binary.

Each wheel places the native binary directly at
`<pkg>-<version>.data/scripts/pq`, which pip installs to `<venv>/bin/pq`
verbatim. There is deliberately no `[console_scripts]` entry point: if one
is declared, pip generates its own Python launcher script at that same
`bin/pq` path and installs it *after* the data-scripts binary, clobbering
the real executable with a shim that does `from pqtool import main` -
which fails, because no `pqtool` Python module is ever packaged. The
`.data/scripts` mechanism alone is sufficient and works unmodified.

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
import sys
import tempfile
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
        # S_IFREG is required, not just the permission bits: pip's installer
        # (zip_item_is_executable in pip._internal.utils.unpacking) only
        # chmod's the extracted file +x if stat.S_ISREG(mode) is true, and
        # S_ISREG inspects the file-type bits, which live above the
        # permission bits and are NOT implied by them. Without S_IFREG here,
        # `unzip`/`zipfile` still *display* this entry as rwxr-xr-x (they
        # don't require the type bits to show permissions), which is why
        # this was easy to miss by inspecting the zip alone - but pip
        # installs the file as a plain non-executable 0o644 file, so the
        # installed `pq` fails with "permission denied" even once the
        # entry_points.txt clobbering (see below) is also fixed.
        bin_info.external_attr = (
            stat.S_IFREG | stat.S_IRWXU | stat.S_IRGRP | stat.S_IXGRP | stat.S_IROTH | stat.S_IXOTH
        ) << 16
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

        # RECORD (must be last, no hash for itself)
        buf = io.StringIO()
        writer = csv.writer(buf)
        for path, digest, size in records:
            writer.writerow([path, f"sha256={digest}", size])
        writer.writerow([f"{dist_info}/RECORD", "", ""])
        whl.writestr(f"{dist_info}/RECORD", buf.getvalue())

    return wheel_path


def zip_item_is_executable(info: zipfile.ZipInfo) -> bool:
    """Mirrors pip._internal.utils.unpacking.zip_item_is_executable exactly.

    This is pip's own predicate for whether it will chmod +x a file it
    extracts from a wheel. It requires the *file-type* bits (S_IFREG), not
    just the permission bits - a wheel builder that sets only permission
    bits (e.g. `0o755 << 16`, omitting `stat.S_IFREG`) passes casual
    inspection with `unzip -l` or `zipfile.ZipInfo` (which both happily
    print "rwxr-xr-x" from permission bits alone) while still installing as
    a non-executable file, because stat.S_ISREG requires the type bits.
    """
    mode = info.external_attr >> 16
    return bool(mode and stat.S_ISREG(mode) and mode & 0o111)


def self_test() -> None:
    """Regression guard for the two ways this builder has shipped a
    `pq` that pip cannot execute:

      1. A `[console_scripts]` entry point (entry_points.txt / top_level.txt)
         that made pip generate its own launcher script at the same `bin/pq`
         path as the real binary, installed *after* it, clobbering it with a
         `from pqtool import main` shim - and no `pqtool` module ever ships.
      2. `.data/scripts/pq`'s external_attr carrying only permission bits,
         not the S_IFREG file-type bit pip's installer requires before it
         will chmod the extracted file executable.

    Exits non-zero and prints what's wrong on failure; does not touch the
    real `dist/` or `pypi/dist/` directories.
    """
    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        binary_path = os.path.join(tmp, "pq-darwin-arm64")
        with open(binary_path, "w") as f:
            f.write("#!/bin/sh\necho SELF-TEST-STUB\n")
        os.chmod(binary_path, 0o755)

        wheel_path = build_wheel(binary_path, "macosx_11_0_arm64", "0.0.0-selftest", tmp)

        with zipfile.ZipFile(wheel_path) as whl:
            names = whl.namelist()
            dist_info = "pqtool-0.0.0-selftest.dist-info"
            data_scripts_pq = "pqtool-0.0.0-selftest.data/scripts/pq"

            for forbidden in (f"{dist_info}/entry_points.txt", f"{dist_info}/top_level.txt"):
                if forbidden in names:
                    failures.append(
                        f"wheel contains {forbidden!r} - this declares a [console_scripts] "
                        f"entry point, which makes pip install its own launcher over "
                        f".data/scripts/pq (see module docstring)"
                    )

            if data_scripts_pq not in names:
                failures.append(f"wheel does not contain {data_scripts_pq!r} at all")
            else:
                info = whl.getinfo(data_scripts_pq)
                if not zip_item_is_executable(info):
                    mode = info.external_attr >> 16
                    failures.append(
                        f"{data_scripts_pq!r} has external_attr mode {oct(mode)}, which pip's "
                        f"own zip_item_is_executable() would NOT chmod +x on install "
                        f"(missing stat.S_IFREG and/or exec permission bits)"
                    )

    if failures:
        print("pypi/build_wheels.py --self-test: FAILED", file=sys.stderr)
        for f in failures:
            print(f"  - {f}", file=sys.stderr)
        sys.exit(1)

    print("pypi/build_wheels.py --self-test: OK (no entry_points.txt/top_level.txt; "
          ".data/scripts/pq present and marked executable per pip's own check)")


def main():
    parser = argparse.ArgumentParser(description="Build pq Python wheels")
    parser.add_argument("--version", help="Required unless --self-test")
    parser.add_argument("--binaries-dir", help="Directory containing pq-<platform> binaries; required unless --self-test")
    parser.add_argument("--out-dir", default="pypi/dist", help="Output directory for wheels")
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="Build a throwaway wheel from a stub binary and assert it is installable by pip "
        "(no console_scripts entry point clobbering the binary; binary is marked executable "
        "per pip's own zip_item_is_executable check). Exits non-zero on failure. "
        "Ignores --version/--binaries-dir/--out-dir.",
    )
    args = parser.parse_args()

    if args.self_test:
        self_test()
        return

    if not args.version or not args.binaries_dir:
        parser.error("--version and --binaries-dir are required unless --self-test is passed")

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
