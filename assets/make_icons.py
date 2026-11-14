"""Draws the built-in sidebar icon sheet (icons.png).

8 columns, 32 px cells, slots in the order of `Icon` in src/icons.rs.
Run: python3 make_icons.py
"""
from PIL import Image, ImageDraw, ImageFont

CELL, COLS, S = 32, 8, 4          # S: supersampling factor
N = CELL * S
BOLD = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"
MONO = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf"
INK = (24, 27, 33, 255)
PAPER = (176, 186, 198, 255)


def shade(c, k):
    return tuple(max(0, min(255, int(v * k))) for v in c[:3]) + (255,)


def page(d, fill):
    """A sheet of paper with a folded corner."""
    l, t, r, b, fold = 22, 10, 106, 118, 30
    d.polygon([(l, t), (r - fold, t), (r, t + fold), (r, b), (l, b)], fill=fill)
    d.polygon([(r - fold, t), (r, t + fold), (r - fold, t + fold)], fill=shade(fill, 0.68))


def label(d, text, y=76, size=None, font=BOLD, fill=INK):
    size = size or (62 if len(text) == 1 else 44)
    f = ImageFont.truetype(font, size)
    d.text((64, y), text, font=f, fill=fill, anchor="mm")


def lettered(text, colour, **kw):
    def draw(d):
        page(d, colour)
        label(d, text, **kw)
    return draw


def folder(d):
    c = (126, 143, 163, 255)
    d.rounded_rectangle([10, 22, 60, 50], 8, fill=shade(c, 0.82))
    d.rounded_rectangle([10, 34, 118, 106], 9, fill=c)


def folder_open(d):
    c = (126, 143, 163, 255)
    d.rounded_rectangle([10, 22, 60, 50], 8, fill=shade(c, 0.72))
    d.rounded_rectangle([10, 34, 108, 106], 9, fill=shade(c, 0.72))
    d.polygon([(28, 56), (124, 56), (106, 106), (10, 106)], fill=shade(c, 1.22))


def plain(d):
    page(d, PAPER)


def text(d):
    page(d, PAPER)
    for y, w in [(50, 40), (66, 62), (82, 62), (98, 46)]:
        d.rounded_rectangle([34, y, 34 + w, y + 7], 3, fill=shade(PAPER, 0.5))


def image(d):
    c = (98, 176, 122, 255)
    d.rounded_rectangle([12, 20, 116, 108], 10, fill=c)
    d.ellipse([28, 32, 50, 54], fill=(250, 232, 150, 255))
    d.polygon([(20, 100), (52, 58), (72, 82), (86, 68), (108, 100)], fill=shade(c, 0.5))


def shell(d):
    d.rounded_rectangle([10, 20, 118, 108], 10, fill=(66, 76, 90, 255))
    d.line([(28, 44), (50, 62), (28, 80)], fill=(132, 220, 140, 255), width=10, joint="curve")
    d.rounded_rectangle([58, 78, 96, 87], 3, fill=(132, 220, 140, 255))


def lock(d):
    c = (214, 180, 96, 255)
    d.arc([36, 14, 92, 78], 180, 360, fill=shade(c, 0.8), width=13)
    d.rectangle([36, 44, 48, 60], fill=shade(c, 0.8))
    d.rectangle([80, 44, 92, 60], fill=shade(c, 0.8))
    d.rounded_rectangle([22, 56, 106, 114], 10, fill=c)
    d.ellipse([55, 72, 73, 90], fill=INK)
    d.rectangle([60, 84, 68, 102], fill=INK)


def git(d):
    c = (232, 104, 76, 255)
    d.line([(40, 30), (40, 98)], fill=c, width=10)
    d.line([(40, 78), (88, 62), (88, 44)], fill=c, width=10, joint="curve")
    for x, y in [(40, 28), (40, 100), (88, 40)]:
        d.ellipse([x - 15, y - 15, x + 15, y + 15], fill=c)
        d.ellipse([x - 6, y - 6, x + 6, y + 6], fill=(0, 0, 0, 0))


def gear(d):
    import math
    c = (150, 160, 172, 255)
    for i in range(8):
        a = i * math.pi / 4
        x, y = 64 + 42 * math.cos(a), 64 + 42 * math.sin(a)
        d.ellipse([x - 13, y - 13, x + 13, y + 13], fill=c)
    d.ellipse([26, 26, 102, 102], fill=c)
    d.ellipse([48, 48, 80, 80], fill=(0, 0, 0, 0))


def shader(d):
    # A lit sphere: light to dark across concentric offsets.
    base = (160, 110, 230)
    for i in range(12):
        k = 0.55 + i * 0.07
        r = 48 - i * 4
        cx, cy = 64 - i * 1.6, 64 - i * 1.6
        d.ellipse([cx - r, cy - r, cx + r, cy + r], fill=shade(base, k))


def audio(d):
    c = (226, 120, 170, 255)
    d.line([(52, 92), (52, 30), (98, 20), (98, 82)], fill=c, width=10, joint="curve")
    d.line([(52, 42), (98, 32)], fill=c, width=10)
    d.ellipse([24, 78, 58, 108], fill=c)
    d.ellipse([70, 68, 104, 98], fill=c)


def archive(d):
    c = (186, 150, 110, 255)
    page(d, c)
    for i, y in enumerate(range(22, 92, 14)):
        x = 44 if i % 2 == 0 else 56
        d.rectangle([x, y, x + 12, y + 9], fill=INK)


def html(d):
    page(d, (228, 124, 84, 255))
    d.line([(54, 56), (36, 76), (54, 96)], fill=INK, width=9, joint="curve")
    d.line([(74, 56), (92, 76), (74, 96)], fill=INK, width=9, joint="curve")


ICONS = [
    folder, folder_open, plain,
    lettered("R", (226, 122, 70, 255)),            # rust
    lettered("T", (164, 150, 132, 255)),           # toml
    lettered("M", (96, 164, 232, 255)),            # markdown
    lettered("{}", (230, 190, 80, 255), font=MONO),  # json
    lettered("Y", (206, 110, 150, 255)),           # yaml
    text, image, shell, lock, git, gear, shader,
    lettered("C", (104, 150, 220, 255)),           # c
    lettered("C+", (84, 128, 204, 255)),           # cpp
    lettered("Py", (104, 168, 208, 255)),          # python
    lettered("JS", (232, 212, 90, 255)),           # javascript
    lettered("TS", (84, 142, 214, 255)),           # typescript
    html,
    lettered("#", (150, 124, 226, 255)),           # css
    audio, archive,
]

rows = -(-len(ICONS) // COLS)
sheet = Image.new("RGBA", (COLS * CELL, rows * CELL), (0, 0, 0, 0))
for i, draw in enumerate(ICONS):
    big = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    draw(ImageDraw.Draw(big))
    sheet.paste(big.resize((CELL, CELL), Image.LANCZOS), ((i % COLS) * CELL, (i // COLS) * CELL))
sheet.save("icons.png")
