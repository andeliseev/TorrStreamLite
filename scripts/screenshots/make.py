#!/usr/bin/env python3
"""Renders the README and website screenshots of the control page, on macOS.

  python3 scripts/screenshots/make.py

The real ui/ files run in WebKit, like in the Mac app, with the sample data in
mock.js, and are saved as docs/images/screenshot-light.png and -dark.png at 2x.
With Pillow installed (pip install pillow) they're reduced to 256 colors, about
a third of the size. The website (andeliseev.github.io/torrstream-lite) uses
the same two files.
"""
from pathlib import Path
import subprocess
import tempfile

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
UI = ROOT / 'ui'
OUT = ROOT / 'docs/images'
# The window buttons, drawn where macOS puts them over the transparent titlebar.
LIGHTS = ('<div aria-hidden="true" style="position:fixed;left:13px;top:8px;display:flex;gap:8px;z-index:30">'
          + ''.join(f'<i style="width:12px;height:12px;border-radius:50%;background:{color};'
                    'box-shadow:inset 0 0 0 .5px rgba(0,0,0,.25)"></i>'
                    for color in ('#ff5f57', '#febc2e', '#28c840'))
          + '</div>')


def demo_page():
    html = (UI / 'index.html').read_text()
    html = html.replace('<link rel="stylesheet" href="/styles.css" />',
                        '<style>\n' + (UI / 'styles.css').read_text() + '\n</style>')
    for name in ('peer-rates.js', 'app.js'):
        tag = f'<script src="/{name}" defer></script>'
        if tag not in html:
            raise SystemExit(f'ui/index.html no longer has {tag}')
        html = html.replace(tag, '<script>\n' + (UI / name).read_text() + '\n</script>')
    html = html.replace('<head>', '<head>\n<script>\n' + (HERE / 'mock.js').read_text() + '\n</script>', 1)
    return html.replace('<body>', '<body>\n' + LIGHTS, 1)


def main():
    with tempfile.TemporaryDirectory(prefix='torrstream-shots-') as tmp:
        tmp = Path(tmp)
        page = tmp / 'demo.html'
        page.write_text(demo_page())
        snap = tmp / 'snap'
        subprocess.run(['xcrun', 'swiftc', '-O', str(HERE / 'snap.swift'), '-o', str(snap),
                        '-framework', 'AppKit', '-framework', 'WebKit'], check=True)
        OUT.mkdir(parents=True, exist_ok=True)
        for theme in ('light', 'dark'):
            raw = tmp / f'{theme}.png'
            subprocess.run([str(snap), str(page), theme, str(raw)], check=True)
            out = OUT / f'screenshot-{theme}.png'
            try:
                from PIL import Image
            except ImportError:
                out.write_bytes(raw.read_bytes())
            else:
                # A 256-color palette suits the flat UI colors and keeps text edges crisp.
                image = Image.open(raw).convert('RGB')
                image.quantize(colors=256, method=Image.Quantize.MEDIANCUT,
                               dither=Image.Dither.NONE).save(out, optimize=True)
            print(f'Wrote {out.relative_to(ROOT)} ({out.stat().st_size // 1024} KiB)')


if __name__ == '__main__':
    main()
