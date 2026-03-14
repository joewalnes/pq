#!/usr/bin/env python3
"""
Build static HTML docs from markdown sources.

Converts docs/src/*.md into docs/build/*.html with minimal inline CSS,
no JavaScript, and a simple nav bar. Requires: pip install markdown

Usage:
    python3 docs/build.py              # outputs to docs/build/
    python3 docs/build.py --serve      # build + start local HTTP server
"""

import http.server
import os
import shutil
import struct
import sys
import zlib

import re

import markdown

# ── Page registry (defines nav order) ────────────────────────────────────────

PAGES = [
    ("index.md",                        "Home"),
    ("viewer.md",                       "Viewer"),
    ("tutorials/getting-started.md",    "Getting Started"),
    ("tutorials/sql-queries.md",        "SQL Queries"),
    ("tutorials/jq-expressions.md",     "jq Expressions"),
    ("tutorials/transformations.md",    "Transformations"),
    ("tutorials/remote-files.md",       "Remote Files"),
    ("cli-reference.md",                "CLI Reference"),
    ("faq.md",                          "FAQ"),
]

# ── HTML template ────────────────────────────────────────────────────────────

TEMPLATE = """\
<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} - pq</title>
<link rel="icon" type="image/png" href="favicon.png">
<style>
:root {{
  --bg: #fff;
  --fg: #1a1a1a;
  --muted: #6b7280;
  --border: #e5e7eb;
  --code-bg: #f3f4f6;
  --pre-bg: #1e1e1e;
  --pre-fg: #d4d4d4;
  --link: #2563eb;
  --nav-bg: #f9fafb;
  --nav-active: #2563eb;
}}
@media (prefers-color-scheme: dark) {{
  :root {{
    --bg: #111827;
    --fg: #e5e7eb;
    --muted: #9ca3af;
    --border: #374151;
    --code-bg: #1f2937;
    --pre-bg: #0d1117;
    --pre-fg: #d4d4d4;
    --link: #60a5fa;
    --nav-bg: #1f2937;
    --nav-active: #60a5fa;
  }}
}}
*, *::before, *::after {{ box-sizing: border-box; }}
body {{
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
  color: var(--fg);
  background: var(--bg);
  margin: 0;
  line-height: 1.6;
}}
nav {{
  background: var(--nav-bg);
  border-bottom: 1px solid var(--border);
  padding: 0.5rem 1rem;
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.15rem;
}}
nav .brand {{
  font-weight: 700;
  color: var(--fg);
  text-decoration: none;
  font-size: 1rem;
  margin-right: 0.5rem;
}}
nav .toggle {{ display: none; }}
nav .burger {{
  display: none;
  cursor: pointer;
  margin-left: auto;
  font-size: 1.4rem;
  color: var(--muted);
  user-select: none;
  line-height: 1;
}}
nav .links {{ display: contents; }}
nav .links a {{
  color: var(--muted);
  text-decoration: none;
  padding: 0.3rem 0.5rem;
  border-radius: 4px;
  font-size: 0.9rem;
}}
nav .links a:hover {{ color: var(--fg); }}
nav .links a.active {{ color: var(--nav-active); font-weight: 600; }}
@media (max-width: 700px) {{
  nav {{ flex-wrap: nowrap; }}
  nav .burger {{ display: block; }}
  nav .links {{
    display: none;
    flex-basis: 100%;
    flex-direction: column;
    padding: 0.4rem 0;
  }}
  nav .links a {{
    padding: 0.45rem 0.5rem;
  }}
  nav .toggle:checked ~ .links {{
    display: flex;
  }}
}}
main {{
  max-width: 52rem;
  margin: 2rem auto;
  padding: 0 1.5rem;
}}
h1, h2 {{ position: relative; }}
h1 {{ font-size: 1.8rem; margin-top: 0; border-bottom: 1px solid var(--border); padding-bottom: 0.4rem; }}
h2 {{ font-size: 1.4rem; margin-top: 2rem; }}
h3 {{ font-size: 1.15rem; margin-top: 1.5rem; }}
h1 .anchor, h2 .anchor {{
  color: var(--muted);
  text-decoration: none;
  font-weight: 400;
  margin-left: 0.3rem;
  opacity: 0;
  font-size: 0.75em;
  transition: opacity 0.15s;
}}
h1:hover .anchor, h2:hover .anchor {{ opacity: 1; }}
.cmds a {{ text-decoration: none; }}
.cmds a code:hover {{
  background: #3a3a3a;
  color: #fff;
  transition: background 0.15s;
}}
a {{ color: var(--link); }}
code {{
  font-family: "SF Mono", Menlo, Consolas, monospace;
  font-size: 0.88em;
  background: var(--code-bg);
  padding: 0.15em 0.35em;
  border-radius: 3px;
}}
pre {{
  background: var(--pre-bg);
  color: var(--pre-fg);
  padding: 1rem;
  border-radius: 6px;
  overflow-x: auto;
  line-height: 1.4;
}}
pre code {{
  background: none;
  padding: 0;
  font-size: 0.85rem;
}}
table {{
  border-collapse: collapse;
  width: 100%;
  margin: 1rem 0;
}}
th, td {{
  border: 1px solid var(--border);
  padding: 0.5rem 0.75rem;
  text-align: left;
}}
th {{ background: var(--code-bg); font-weight: 600; }}
blockquote {{
  border-left: 3px solid var(--border);
  margin: 1rem 0;
  padding: 0.5rem 1rem;
  color: var(--muted);
}}
.features {{
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 0.75rem;
  margin: 1.5rem 0;
}}
.feature {{
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 1rem 1rem 0.9rem;
  display: flex;
  gap: 0.8rem;
  align-items: flex-start;
}}
.feature-icon {{
  font-size: 1.5rem;
  line-height: 1;
  flex-shrink: 0;
  padding-top: 0.1rem;
}}
.feature-body h3 {{
  margin: 0 0 0.2rem 0;
  font-size: 0.95rem;
}}
.feature-body p {{
  margin: 0 0 0.4rem 0;
  font-size: 0.85rem;
  color: var(--muted);
  line-height: 1.45;
}}
.feature-body .cmds {{
  display: flex;
  flex-wrap: wrap;
  gap: 0.3rem;
}}
.feature-body .cmds code {{
  font-size: 0.78rem;
  background: var(--pre-bg);
  color: var(--pre-fg);
  padding: 0.15em 0.5em;
  border-radius: 3px;
}}
@media (max-width: 600px) {{
  main {{ padding: 0 1rem; }}
  pre {{ font-size: 0.8rem; padding: 0.75rem; }}
  .features {{ grid-template-columns: 1fr; }}
}}
</style>
</head>
<body>
<nav>
<a class="brand" href=".">pq</a>
<input type="checkbox" id="nav-toggle" class="toggle">
<label for="nav-toggle" class="burger" aria-label="Menu">&#9776;</label>
<div class="links">
{nav}
</div>
</nav>
<main>
{content}
</main>
</body>
</html>
"""

# ── Build logic ──────────────────────────────────────────────────────────────

SRC_DIR = os.path.join(os.path.dirname(__file__), "src")
BUILD_DIR = os.path.join(os.path.dirname(__file__), "build")

MD = markdown.Markdown(extensions=["fenced_code", "tables"])


def generate_favicon(path: str) -> None:
    """Generate a 32x32 pixel-art terminal favicon as PNG."""
    W, H = 32, 32

    # Colours (RGBA) matching the site palette.
    T  = (0, 0, 0, 0)                        # transparent (corners)
    BG = (0x1E, 0x1E, 0x1E, 0xFF)            # window body  (--pre-bg)
    TB = (0x2D, 0x2D, 0x2D, 0xFF)            # title bar
    RD = (0xFF, 0x5F, 0x57, 0xFF)            # close dot
    YL = (0xFE, 0xBC, 0x2E, 0xFF)            # minimise dot
    GN = (0x28, 0xC8, 0x40, 0xFF)            # maximise dot
    TX = (0x25, 0x63, 0xEB, 0xFF)            # text  (--link)

    img = [[BG] * W for _ in range(H)]

    # ── Title bar (rows 0-7) ──────────────────────────────────────────────
    for y in range(8):
        for x in range(W):
            img[y][x] = TB

    # Rounded corners (top).
    for x, y in [(0, 0), (1, 0), (0, 1), (30, 0), (31, 0), (31, 1)]:
        img[y][x] = T
    # Rounded corners (bottom).
    for x, y in [(0, 30), (0, 31), (1, 31), (30, 31), (31, 31), (31, 30)]:
        img[y][x] = T

    # Traffic-light dots (4x4 rounded).
    dot = [(1, 0), (2, 0),
           (0, 1), (1, 1), (2, 1), (3, 1),
           (0, 2), (1, 2), (2, 2), (3, 2),
           (1, 3), (2, 3)]
    for color, bx in [(RD, 3), (YL, 9), (GN, 15)]:
        for dx, dy in dot:
            img[2 + dy][bx + dx] = color

    # ── Slab-serif "pq" ──────────────────────────────────────────────────
    # Each letter: 11px wide x 22px tall, built from filled rectangles.
    # 3px strokes, geometric bowls, slab serifs on descender terminals.
    # Rectangles are (x, y, w, h) relative to the letter origin.
    p_rects = [
        (0, 0, 11, 2),    # bowl top bar
        (0, 2, 3, 8),     # bowl left side (= stem upper)
        (8, 2, 3, 8),     # bowl right side
        (0, 10, 11, 2),   # bowl bottom bar
        (0, 12, 3, 8),    # stem lower
        (-2, 20, 7, 2),   # slab serif (extends into margin)
    ]
    q_rects = [
        (0, 0, 11, 2),    # bowl top bar
        (0, 2, 3, 8),     # bowl left side
        (8, 2, 3, 8),     # bowl right side (= stem upper)
        (0, 10, 11, 2),   # bowl bottom bar
        (8, 12, 3, 8),    # stem lower
        (6, 20, 7, 2),    # slab serif (extends into margin)
    ]

    def fill(rects, ox, oy):
        for rx, ry, rw, rh in rects:
            for dy in range(rh):
                for dx in range(rw):
                    px, py = ox + rx + dx, oy + ry + dy
                    if 0 <= px < W and 0 <= py < H:
                        img[py][px] = TX

    fill(p_rects, 3, 9)     # p: 3px left margin
    fill(q_rects, 18, 9)    # q: 3+11+4=18  (4px inter-letter gap)

    # ── Encode as PNG (stdlib only, no Pillow) ────────────────────────────
    def chunk(tag: bytes, data: bytes) -> bytes:
        c = tag + data
        crc = zlib.crc32(c) & 0xFFFFFFFF
        return struct.pack(">I", len(data)) + c + struct.pack(">I", crc)

    raw = b""
    for row in img:
        raw += b"\x00"                        # filter byte: None
        for r, g, b, a in row:
            raw += bytes((r, g, b, a))

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", W, H, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(raw))
    png += chunk(b"IEND", b"")

    with open(path, "wb") as f:
        f.write(png)


def md_to_html(src_path: str) -> tuple[str, str]:
    """Convert a markdown file to HTML. Returns (title, html_body)."""
    with open(src_path, "r") as f:
        text = f.read()

    MD.reset()
    body = MD.convert(text)

    # Extract title from first <h1>.
    title = "pq"
    for line in text.splitlines():
        if line.startswith("# "):
            title = line[2:].strip()
            break

    # Add id anchors to h1 and h2 headings with a discoverable link icon.
    def add_anchor(m: re.Match) -> str:
        tag = m.group(1)
        text = m.group(2)
        slug = re.sub(r"[^\w\s-]", "", text.lower())
        slug = re.sub(r"[\s]+", "-", slug).strip("-")
        return f'<{tag} id="{slug}">{text} <a class="anchor" href="#{slug}">#</a></{tag}>'

    body = re.sub(r"<(h[12])>(.*?)</\1>", add_anchor, body)

    # Rewrite .md links to .html and flatten paths (tutorials/foo.md -> tutorials-foo.html)
    # so links work in the built site while keeping .md links valid on GitHub.
    def rewrite_link(m: re.Match) -> str:
        url = m.group(1)
        url = url.removeprefix("./")
        url = url.replace("/", "-")
        url = url.removesuffix(".md") + ".html"
        return f'href="{url}"'

    body = re.sub(r'href="([^"]*?\.md)"', rewrite_link, body)

    return title, body


def html_path(md_name: str) -> str:
    """Map a source .md filename to an output .html filename (flat)."""
    # Flatten: tutorials/getting-started.md -> tutorials-getting-started.html
    return md_name.replace("/", "-").removesuffix(".md") + ".html"


def build_nav(active_md: str) -> str:
    """Build the nav bar HTML."""
    links = []
    for md_name, label in PAGES:
        href = html_path(md_name)
        if href == "index.html":
            href = "."
        cls = ' class="active"' if md_name == active_md else ""
        links.append(f'<a href="{href}"{cls}>{label}</a>')
    return "\n".join(links)


def build() -> None:
    # Remove old HTML files but preserve img/ (demo GIFs are built separately)
    if os.path.exists(BUILD_DIR):
        for f in os.listdir(BUILD_DIR):
            path = os.path.join(BUILD_DIR, f)
            if os.path.isfile(path):
                os.remove(path)
    os.makedirs(BUILD_DIR, exist_ok=True)

    for md_name, _label in PAGES:
        src_path = os.path.join(SRC_DIR, md_name)
        if not os.path.exists(src_path):
            print(f"  warning: {src_path} not found, skipping", file=sys.stderr)
            continue

        title, body = md_to_html(src_path)
        nav = build_nav(md_name)
        html = TEMPLATE.format(title=title, nav=nav, content=body)

        out_path = os.path.join(BUILD_DIR, html_path(md_name))
        with open(out_path, "w") as f:
            f.write(html)

    # Generate pixel-art favicon.
    generate_favicon(os.path.join(BUILD_DIR, "favicon.png"))

    print(f"Built {len(PAGES)} pages in {BUILD_DIR}/")


def serve(port: int = 8000) -> None:
    os.chdir(BUILD_DIR)
    handler = http.server.SimpleHTTPRequestHandler
    with http.server.HTTPServer(("", port), handler) as httpd:
        print(f"Serving {BUILD_DIR}/ at http://localhost:{port}")
        httpd.serve_forever()


def main() -> None:
    build()
    if "--serve" in sys.argv:
        serve()


if __name__ == "__main__":
    main()
