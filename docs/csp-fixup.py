#!/usr/bin/env python3
"""
Post-process mdBook HTML output to work under strict CSP.

mdBook uses inline <script> tags which are blocked by
Content-Security-Policy: script-src 'self'. This script extracts all
inline scripts into external .js files and replaces them with <script src="...">
references.

Usage:
    python3 docs/csp-fixup.py          # processes docs/book/
    python3 docs/csp-fixup.py [DIR]    # processes DIR/
"""

import hashlib
import os
import re
import sys

INLINE_SCRIPT = re.compile(r"<script>(.*?)</script>", re.DOTALL)


def process_file(html_path: str, book_root: str) -> int:
    with open(html_path, "r") as f:
        html = f.read()

    scripts = list(INLINE_SCRIPT.finditer(html))
    if not scripts:
        return 0

    # Combine all inline scripts into one external file per page.
    combined = "\n".join(m.group(1) for m in scripts)
    digest = hashlib.sha256(combined.encode()).hexdigest()[:12]

    rel = os.path.relpath(html_path, book_root)
    base = rel.replace(os.sep, "-").removesuffix(".html")
    js_name = f"_csp/{base}-{digest}.js"
    js_path = os.path.join(book_root, js_name)

    os.makedirs(os.path.dirname(js_path), exist_ok=True)
    with open(js_path, "w") as f:
        f.write(combined)

    # Compute relative path from this HTML file to the JS file.
    html_dir = os.path.dirname(html_path)
    rel_js = os.path.relpath(js_path, html_dir)

    # Replace: first inline script becomes the external reference,
    # remaining inline scripts are removed.
    first = True

    def replacer(m: re.Match) -> str:
        nonlocal first
        if first:
            first = False
            return f'<script src="{rel_js}"></script>'
        return ""

    html = INLINE_SCRIPT.sub(replacer, html)

    with open(html_path, "w") as f:
        f.write(html)

    return len(scripts)


def main() -> None:
    book_dir = sys.argv[1] if len(sys.argv) > 1 else "docs/book"

    if not os.path.isdir(book_dir):
        print(f"error: {book_dir} not found (run mdbook build first)", file=sys.stderr)
        sys.exit(1)

    total_files = 0
    total_scripts = 0

    for root, _dirs, files in os.walk(book_dir):
        for name in files:
            if not name.endswith(".html"):
                continue
            path = os.path.join(root, name)
            n = process_file(path, book_dir)
            if n:
                total_files += 1
                total_scripts += n

    print(f"Extracted {total_scripts} inline scripts from {total_files} files into {book_dir}/_csp/")


if __name__ == "__main__":
    main()
