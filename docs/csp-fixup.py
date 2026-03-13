#!/usr/bin/env python3
"""
Post-process mdBook HTML output to work under strict CSP.

mdBook uses inline <script> tags which are blocked by
Content-Security-Policy: script-src 'self'. This script extracts all
inline scripts into external .js files and replaces them with <script src="...">
references.

Scripts that appear in <head> and <body> are kept separate so that
body-dependent code doesn't run before the DOM is ready.

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

    # Split scripts into head vs body based on position relative to <body>.
    body_start = html.find("<body")
    head_scripts = []
    body_scripts = []
    for m in scripts:
        if m.start() < body_start:
            head_scripts.append(m)
        else:
            body_scripts.append(m)

    rel = os.path.relpath(html_path, book_root)
    base = rel.replace(os.sep, "-").removesuffix(".html")
    html_dir = os.path.dirname(html_path)
    os.makedirs(os.path.join(book_root, "_csp"), exist_ok=True)

    js_files = {}  # match object -> (js_path, rel_js)

    for label, group in [("head", head_scripts), ("body", body_scripts)]:
        if not group:
            continue
        combined = "\n".join(m.group(1) for m in group)
        digest = hashlib.sha256(combined.encode()).hexdigest()[:12]
        js_name = f"_csp/{base}-{label}-{digest}.js"
        js_path = os.path.join(book_root, js_name)
        rel_js = os.path.relpath(js_path, html_dir)

        with open(js_path, "w") as f:
            f.write(combined)

        # Tag the first match in this group with the external reference.
        js_files[group[0].start()] = rel_js

    # Replace inline scripts: first in each group becomes external ref,
    # rest are removed.
    def replacer(m: re.Match) -> str:
        if m.start() in js_files:
            return f'<script src="{js_files[m.start()]}"></script>'
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
