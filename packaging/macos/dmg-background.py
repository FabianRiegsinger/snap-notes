"""Draws the .dmg window background: dmg-background.png and @2x.

Run from the repo root after changing the look (needs Pillow):
    python3 packaging/macos/dmg-background.py
The icon spots must match icon_locations in dmg-settings.py.
"""

import math

from PIL import Image, ImageDraw, ImageFilter, ImageFont

W, H = 660, 420
APP, APPS = (180, 235), (480, 235)
INK = (48, 44, 40)
# The default note palette: Coral, Peach, Amber, Mint, Sky, Lavender.
NOTES = ["#FF6B6B", "#FFA07A", "#FFD93D", "#6BCB77", "#45B7D1", "#B983FF"]
FONTS = "assets/fonts"


def draw(out_scale):
    # Drawn 3x larger, then scaled down, for smooth edges.
    scale = out_scale * 3
    s = lambda v: round(v * scale)
    w, h = s(W), s(H)

    # Warm paper gradient, top to bottom.
    top, bottom = (253, 250, 246), (244, 234, 224)
    img = Image.new("RGB", (w, h))
    px = ImageDraw.Draw(img)
    for y in range(h):
        t = y / (h - 1)
        px.line([(0, y), (w, y)], fill=tuple(round(a + (b - a) * t) for a, b in zip(top, bottom)))

    # Soft colored glows behind each icon spot.
    img = img.convert("RGBA")
    for (cx, cy), color, alpha in [(APP, (255, 160, 122), 90), (APPS, (69, 183, 209), 55)]:
        mask = Image.new("L", (w, h), 0)
        r = s(105)
        ImageDraw.Draw(mask).ellipse([s(cx) - r, s(cy) - r, s(cx) + r, s(cy) + r], fill=alpha)
        mask = mask.filter(ImageFilter.GaussianBlur(s(45)))
        img.paste(Image.new("RGBA", (w, h), color + (255,)), mask=mask)

    # Translucent ink goes on its own layer so it blends.
    layer = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    solid = ImageDraw.Draw(img)

    # The app's own look: a slim strip of note bars along the right edge.
    bar_w, bar_h, gap = s(6), s(34), s(6)
    x1 = w - s(14)
    y = (h - (len(NOTES) * bar_h + (len(NOTES) - 1) * gap)) // 2
    for color in NOTES:
        solid.rounded_rectangle([x1 - bar_w, y, x1, y + bar_h], radius=bar_w // 2, fill=color)
        y += bar_h + gap

    # Title and hint.
    title = ImageFont.truetype(f"{FONTS}/Inter-Bold.subset.ttf", s(26))
    hint = ImageFont.truetype(f"{FONTS}/Inter-Regular.subset.ttf", s(14))
    d.text((w // 2, s(62)), "Snap Notes", font=title, fill=INK + (255,), anchor="mm")
    d.text((w // 2, s(94)), "Drag the app into Applications to install it.",
           font=hint, fill=INK + (150,), anchor="mm")

    # Arrow from the app to Applications: a gentle arc with a rounded head.
    ink = INK + (110,)
    x0, x2, cy = s(APP[0] + 82), s(APPS[0] - 82), s(APP[1] - 8)
    lift = s(22)
    points = []
    for i in range(41):
        t = i / 40
        points.append((x0 + (x2 - x0) * t, cy - lift * 4 * t * (1 - t)))
    width = s(4)
    d.line(points, fill=ink, width=width, joint="curve")
    for x, yy in (points[0], points[-1]):
        d.ellipse([x - width / 2, yy - width / 2, x + width / 2, yy + width / 2], fill=ink)
    head = s(12)
    tip, before = points[-1], points[-3]
    angle = math.atan2(tip[1] - before[1], tip[0] - before[0])
    for side in (-1, 1):
        a = angle + math.pi + side * math.radians(38)
        end = (tip[0] + head * math.cos(a), tip[1] + head * math.sin(a))
        d.line([tip, end], fill=ink, width=width)
        d.ellipse([end[0] - width / 2, end[1] - width / 2, end[0] + width / 2, end[1] + width / 2], fill=ink)
    d.ellipse([tip[0] - width / 2, tip[1] - width / 2, tip[0] + width / 2, tip[1] + width / 2], fill=ink)

    img = Image.alpha_composite(img, layer).convert("RGB")
    return img.resize((W * out_scale, H * out_scale), Image.LANCZOS)


draw(1).save("packaging/macos/dmg-background.png")
draw(2).save("packaging/macos/dmg-background@2x.png")
