"""Material Forge starter alphas and textures, generated (clean-room: no
image from any other product is used; shapes and noises are computed here).

usage: python -E -P gen_assets.py <starter_assets dir>
Alphas: 512 px grayscale PNG (white paints). Textures: 1024 px grayscale
PNG, tileable (periodic noises), heights for the stamps (white is high).
"""
import math, os, sys
import numpy as np
from PIL import Image, ImageDraw, ImageFilter, ImageFont

OUT = sys.argv[1]
A = 512
T = 1024
rng = np.random.default_rng(20261023)


def save(img, kind, cat, name):
    d = os.path.join(OUT, kind, cat)
    os.makedirs(d, exist_ok=True)
    if isinstance(img, np.ndarray):
        img = Image.fromarray((np.clip(img, 0, 1) * 255 + 0.5).astype(np.uint8), 'L')
    img.save(os.path.join(d, name + '.png'), optimize=True)


# ------------------------------------------------------------ helpers
def grid(n):
    y, x = np.mgrid[0:n, 0:n].astype(np.float32)
    return (x + 0.5) / n * 2 - 1, (y + 0.5) / n * 2 - 1


def smooth(e, w):
    """Antialiased step from a signed distance (in pixels)."""
    return np.clip(0.5 - e / w, 0, 1)


def polar(n=A):
    x, y = grid(n)
    return x, y, np.sqrt(x * x + y * y), np.arctan2(y, x)


def fbm(n, beta=2.0, seed=None, octaves=None):
    """Periodic (tileable) noise with a 1/f^beta spectrum, 0..1."""
    r = np.random.default_rng(seed) if seed is not None else rng
    fx = np.fft.fftfreq(n)[None, :]
    fy = np.fft.fftfreq(n)[:, None]
    f = np.sqrt(fx * fx + fy * fy)
    f[0, 0] = 1
    amp = f ** (-beta / 2.0)
    amp[0, 0] = 0
    if octaves is not None:
        amp[f < octaves[0] / n] = 0
        amp[f > octaves[1] / n] = 0
    ph = r.uniform(0, 2 * np.pi, (n, n))
    v = np.real(np.fft.ifft2(amp * np.exp(1j * ph)))
    v = (v - v.mean()) / (v.std() + 1e-9)
    return np.clip(v * 0.18 + 0.5, 0, 1)


def blur(a, r):
    """Periodic gaussian blur through the FFT."""
    n = a.shape[0]
    fx = np.fft.fftfreq(n)[None, :]
    fy = np.fft.fftfreq(n)[:, None]
    g = np.exp(-2 * (np.pi * r) ** 2 * (fx * fx + fy * fy))
    return np.real(np.fft.ifft2(np.fft.fft2(a) * g))


def levels(a, lo, hi):
    return np.clip((a - lo) / max(hi - lo, 1e-6), 0, 1)


def worley(n, cells, seed=0, f2=False):
    r = np.random.default_rng(seed)
    pts = r.uniform(0, 1, (cells * cells, 2))
    y, x = np.mgrid[0:n, 0:n].astype(np.float32) / n
    d1 = np.full((n, n), 9.0, np.float32)
    d2 = np.full((n, n), 9.0, np.float32)
    for p in pts:
        dx = np.abs(x - p[0]); dx = np.minimum(dx, 1 - dx)
        dy = np.abs(y - p[1]); dy = np.minimum(dy, 1 - dy)
        d = np.sqrt(dx * dx + dy * dy)
        d2 = np.where(d < d1, d1, np.minimum(d2, d))
        d1 = np.minimum(d1, d)
    s = cells
    return (d2 - d1) * s if f2 else d1 * s


def hexd(px, py):
    """Hex grid: distance from the cell center (0) to its edge (0.5)."""
    r = np.array([1.0, math.sqrt(3.0)])
    h = r * 0.5
    ax = np.mod(px, r[0]) - h[0]; ay = np.mod(py, r[1]) - h[1]
    bx = np.mod(px - h[0], r[0]) - h[0]; by = np.mod(py - h[1], r[1]) - h[1]
    use_a = ax * ax + ay * ay < bx * bx + by * by
    gx = np.where(use_a, ax, bx); gy = np.where(use_a, ay, by)
    return np.maximum(np.abs(gx), np.abs(gx) * 0.5 + np.abs(gy) * 0.866)


def stamp_canvas(scale=4, n=A):
    im = Image.new('L', (n * scale, n * scale), 0)
    return im, ImageDraw.Draw(im), scale


def down(im, n=A):
    return im.resize((n, n), Image.LANCZOS)


def poly(cx, cy, r, k, rot=0.0):
    return [(cx + r * math.cos(rot + 2 * math.pi * i / k), cy + r * math.sin(rot + 2 * math.pi * i / k)) for i in range(k)]


def star(cx, cy, r1, r2, k, rot=-math.pi / 2):
    pts = []
    for i in range(2 * k):
        r = r1 if i % 2 == 0 else r2
        a = rot + math.pi * i / k
        pts.append((cx + r * math.cos(a), cy + r * math.sin(a)))
    return pts


def falloff(img, soft=0.06):
    """Fades an alpha toward its edges (a soft round border)."""
    x, y, r, _ = polar(img.shape[0])
    return img * smooth(r - (1 - soft), soft * 2)


# ------------------------------------------------------------ alphas: shapes
def shapes():
    x, y, r, t = polar()
    w = 2.0 / A
    out = {}
    out['Circle Hard'] = smooth(r - 0.92, w)
    out['Circle Soft'] = np.clip(1 - r, 0, 1) ** 1.6
    out['Circle Feather'] = np.exp(-(r / 0.45) ** 2)
    out['Ring'] = smooth(np.abs(r - 0.78) - 0.12, w)
    out['Ring Thin'] = smooth(np.abs(r - 0.85) - 0.04, w)
    out['Ring Double'] = np.maximum(smooth(np.abs(r - 0.85) - 0.05, w), smooth(np.abs(r - 0.55) - 0.05, w))
    sq = np.maximum(np.abs(x), np.abs(y))
    out['Square'] = smooth(sq - 0.86, w)
    out['Square Outline'] = smooth(np.abs(sq - 0.8) - 0.08, w)
    q = np.sqrt(np.maximum(np.abs(x) - 0.55, 0) ** 2 + np.maximum(np.abs(y) - 0.55, 0) ** 2)
    out['Rounded Square'] = smooth(q - 0.32, w)
    out['Diamond'] = smooth(np.abs(x) + np.abs(y) - 0.9, w)
    out['Bar'] = smooth(np.maximum(np.abs(x) - 0.9, np.abs(y) - 0.18), w)
    out['Half Circle'] = smooth(r - 0.9, w) * smooth(-y, w)
    out['Crescent'] = smooth(r - 0.88, w) * (1 - smooth(np.sqrt((x - 0.35) ** 2 + (y + 0.15) ** 2) - 0.75, w))
    out['Dot Grid'] = smooth(np.sqrt(((x * 1.5 + 0.5) % 1 - 0.5) ** 2 + ((y * 1.5 + 0.5) % 1 - 0.5) ** 2) * 2 - 0.55, 4 * w) * smooth(sq - 0.95, w)
    for name in out:
        save(out[name], 'alphas', 'Shapes', name)
    # Polygon shapes, drawn at 4x and reduced (antialiased).
    defs = {
        'Triangle': lambda d, s: d.polygon(poly(256 * s, 280 * s, 230 * s, 3, -math.pi / 2), fill=255),
        'Triangle Outline': lambda d, s: (d.polygon(poly(256 * s, 280 * s, 235 * s, 3, -math.pi / 2), fill=255), d.polygon(poly(256 * s, 280 * s, 165 * s, 3, -math.pi / 2), fill=0)),
        'Hexagon': lambda d, s: d.polygon(poly(256 * s, 256 * s, 235 * s, 6), fill=255),
        'Hexagon Outline': lambda d, s: (d.polygon(poly(256 * s, 256 * s, 235 * s, 6), fill=255), d.polygon(poly(256 * s, 256 * s, 185 * s, 6), fill=0)),
        'Pentagon': lambda d, s: d.polygon(poly(256 * s, 270 * s, 230 * s, 5, -math.pi / 2), fill=255),
        'Octagon': lambda d, s: d.polygon(poly(256 * s, 256 * s, 235 * s, 8, math.pi / 8), fill=255),
        'Star': lambda d, s: d.polygon(star(256 * s, 270 * s, 240 * s, 100 * s, 5), fill=255),
        'Star Six': lambda d, s: d.polygon(star(256 * s, 256 * s, 240 * s, 130 * s, 6), fill=255),
        'Star Burst': lambda d, s: d.polygon(star(256 * s, 256 * s, 245 * s, 120 * s, 12), fill=255),
        'Plus': lambda d, s: (d.rectangle([186 * s, 30 * s, 326 * s, 482 * s], fill=255), d.rectangle([30 * s, 186 * s, 482 * s, 326 * s], fill=255)),
        'Cross': lambda d, s: (d.line([60 * s, 60 * s, 452 * s, 452 * s], fill=255, width=110 * s), d.line([452 * s, 60 * s, 60 * s, 452 * s], fill=255, width=110 * s)),
        'Arrow': lambda d, s: d.polygon([(40 * s, 200 * s), (300 * s, 200 * s), (300 * s, 90 * s), (480 * s, 256 * s), (300 * s, 422 * s), (300 * s, 312 * s), (40 * s, 312 * s)], fill=255),
        'Chevron': lambda d, s: d.polygon([(90 * s, 60 * s), (250 * s, 60 * s), (430 * s, 256 * s), (250 * s, 452 * s), (90 * s, 452 * s), (270 * s, 256 * s)], fill=255),
        'Heart': lambda d, s: d.polygon([(256 * s + 230 * s * (16 * math.sin(t) ** 3) / 17, 236 * s - 230 * s * (13 * math.cos(t) - 5 * math.cos(2 * t) - 2 * math.cos(3 * t) - math.cos(4 * t)) / 17) for t in np.linspace(0, 2 * math.pi, 200)], fill=255),
        'Lightning': lambda d, s: d.polygon([(290 * s, 20 * s), (110 * s, 290 * s), (240 * s, 290 * s), (200 * s, 492 * s), (400 * s, 200 * s), (270 * s, 200 * s)], fill=255),
        'Drop': lambda d, s: (d.ellipse([116 * s, 210 * s, 396 * s, 490 * s], fill=255), d.polygon([(256 * s, 20 * s), (122 * s, 320 * s), (390 * s, 320 * s)], fill=255)),
        'Gear': lambda d, s: (d.polygon(star(256 * s, 256 * s, 240 * s, 190 * s, 10, 0), fill=255), d.ellipse([186 * s, 186 * s, 326 * s, 326 * s], fill=0)),
        'Flower': lambda d, s: [d.ellipse([(256 + 130 * math.cos(a) - 105) * s, (256 + 130 * math.sin(a) - 105) * s, (256 + 130 * math.cos(a) + 105) * s, (256 + 130 * math.sin(a) + 105) * s], fill=255) for a in np.linspace(0, 2 * math.pi, 6)[:-1]] + [d.ellipse([196 * s, 196 * s, 316 * s, 316 * s], fill=255)],
    }
    for name, f in defs.items():
        im, d, s = stamp_canvas()
        f(d, s)
        save(down(im), 'alphas', 'Shapes', name)


# ------------------------------------------------------------ alphas: stitches, seams, hardware
def stitches():
    w = 2.0 / A
    x, y, r, t = polar()
    # Stitches are drawn horizontally: the path tool turns them along the path.
    def capsule(cx, cy, hl, hr, ang=0.0):
        c, s_ = math.cos(ang), math.sin(ang)
        lx = (x - cx) * c + (y - cy) * s_
        ly = -(x - cx) * s_ + (y - cy) * c
        d = np.sqrt(np.maximum(np.abs(lx) - hl, 0) ** 2 + ly * ly) - hr
        return d
    def thread(d, hr):
        # A rounded thread: bright core, darker rim (reads as a height too).
        inside = smooth(d, w)
        core = np.clip(1 - (d + hr) / hr, 0, 1)
        return inside * (0.55 + 0.45 * np.sqrt(core))
    out = {}
    out['Stitch'] = thread(capsule(0, 0, 0.55, 0.16), 0.16)
    out['Stitch Slanted'] = thread(capsule(0, 0, 0.5, 0.15, 0.5), 0.15)
    out['Stitch Double'] = np.maximum(thread(capsule(0, -0.38, 0.55, 0.12), 0.12), thread(capsule(0, 0.38, 0.55, 0.12), 0.12))
    out['Cross Stitch'] = np.maximum(thread(capsule(0, 0, 0.6, 0.13, 0.785), 0.13), thread(capsule(0, 0, 0.6, 0.13, -0.785), 0.13))
    out['Zigzag Stitch'] = np.maximum(thread(capsule(-0.4, 0, 0.45, 0.12, 1.0), 0.12), thread(capsule(0.4, 0, 0.45, 0.12, -1.0), 0.12))
    out['Blanket Stitch'] = np.maximum(thread(capsule(0, 0.45, 0.7, 0.1), 0.1), thread(capsule(0, -0.05, 0.5, 0.1, math.pi / 2), 0.1))
    out['Dash'] = smooth(np.maximum(np.abs(x) - 0.7, np.abs(y) - 0.12), w)
    out['Dots Line'] = smooth(np.sqrt(((x * 2 + 1) % 1 - 0.5) ** 2 + y * y * 4) - 0.3, 4 * w)
    # Zipper tooth: a rounded block with a groove.
    tooth = smooth(np.sqrt(np.maximum(np.abs(x) - 0.22, 0) ** 2 + np.maximum(np.abs(y + 0.1) - 0.45, 0) ** 2) - 0.18, w)
    groove = 1 - 0.5 * smooth(np.abs(y + 0.1) - 0.05, w) * smooth(np.abs(x) - 0.3, w)
    out['Zipper Tooth'] = tooth * groove * (0.6 + 0.4 * (1 - np.abs(x) / 0.45).clip(0, 1))
    # Hardware, as heights: domes, heads, slots.
    dome = np.sqrt(np.clip(1 - (r / 0.85) ** 2, 0, 1))
    out['Rivet'] = dome * smooth(r - 0.85, w)
    out['Rivet Flat'] = smooth(r - 0.85, w) * (0.7 + 0.3 * np.clip(1 - r / 0.85, 0, 1))
    head = dome * smooth(r - 0.88, w)
    out['Screw Slotted'] = head * (1 - 0.8 * smooth(np.maximum(np.abs(y) - 0.07, np.abs(x) - 0.62), w))
    out['Screw Cross'] = head * (1 - 0.8 * np.maximum(smooth(np.maximum(np.abs(y) - 0.07, np.abs(x) - 0.5), w), smooth(np.maximum(np.abs(x) - 0.07, np.abs(y) - 0.5), w)))
    hexb = np.maximum(np.abs(x) * 0.866 + np.abs(y) * 0.5, np.abs(y))
    out['Bolt Hex'] = smooth(hexb - 0.82, w) * (0.75 + 0.25 * smooth(r - 0.45, 6 * w))
    # A sharp V profile (paint it with a negative height); the ridge is round.
    out['Seam Groove'] = np.clip(1 - np.abs(y) / 0.09, 0, 1)
    out['Seam Ridge'] = np.exp(-(y / 0.12) ** 2)
    chain = np.maximum(smooth(np.abs(capsule(-0.35, 0, 0.3, 0.32)) - 0.07, w), smooth(np.abs(capsule(0.35, 0, 0.3, 0.32, 0)) - 0.07, w) * 0)
    out['Chain Link'] = smooth(np.abs(capsule(0, 0, 0.45, 0.35)) - 0.09, w)
    # Tire tread block: two slanted bars.
    out['Tread'] = np.maximum(smooth(capsule(-0.35, 0, 0.6, 0.14, 1.1), w), smooth(capsule(0.35, 0, 0.6, 0.14, -1.1), w))
    out['Perforation'] = smooth(r - 0.35, w)
    for name, img in out.items():
        save(img, 'alphas', 'Stitches and hardware', name)


# ------------------------------------------------------------ alphas: grunge
def splatter(seed, blobs=90, spread=0.75, size=(0.01, 0.08), droplets=160):
    r_ = np.random.default_rng(seed)
    im, d, s = stamp_canvas(2)
    c = 256 * s
    for _ in range(blobs):
        a = r_.uniform(0, 2 * math.pi)
        dist = abs(r_.normal(0, spread * 0.45)) * 256 * s
        rad = r_.uniform(*size) * 512 * s * (1.4 - min(dist / (256 * s), 1.0))
        px, py = c + dist * math.cos(a), c + dist * math.sin(a)
        d.ellipse([px - rad, py - rad, px + rad, py + rad], fill=255)
    for _ in range(droplets):
        a = r_.uniform(0, 2 * math.pi)
        dist = r_.uniform(0.3, 0.98) * 256 * s
        rad = r_.uniform(0.6, 4.5) * s
        px, py = c + dist * math.cos(a), c + dist * math.sin(a)
        d.ellipse([px - rad, py - rad, px + rad, py + rad], fill=255)
    return np.asarray(down(im.filter(ImageFilter.GaussianBlur(1.5 * s)))).astype(np.float32) / 255.0


def grunge_alphas():
    x, y, r, t = polar()
    out = {}
    for i in range(4):
        out['Splatter %d' % (i + 1)] = levels(splatter(100 + i, blobs=60 + 30 * i), 0.45, 0.6)
    n = fbm(A, 1.6, seed=7)
    out['Spray'] = blur((rng.random((A, A)) < 0.16 * np.exp(-(r / 0.6) ** 2)).astype(np.float32), 0.6) * 1.6
    out['Spray Dense'] = (rng.random((A, A)) < 0.22 * np.exp(-(r / 0.7) ** 2)).astype(np.float32)
    speck = blur((rng.random((A, A)) < 0.004).astype(np.float32), 1.6)
    out['Speckles'] = levels(speck, 0.02, 0.12) * smooth(r - 0.95, 0.08)
    out['Dirt Spots'] = falloff(levels(fbm(A, 2.3, seed=11) * (0.6 + 0.4 * fbm(A, 1.2, seed=12)), 0.5, 0.75), 0.3)
    out['Smudge'] = falloff(levels(blur(fbm(A, 2.0, seed=13), 6) * np.exp(-(r / 0.75) ** 2) * 1.7, 0.35, 0.95), 0.2)
    out['Stain'] = smooth(r * (1 + 0.18 * (fbm(A, 2.0, seed=14) - 0.5)) - 0.75, 0.05) * (0.35 + 0.65 * smooth(0.6 - r, 0.25))
    out['Water Drop Mark'] = np.clip(smooth(r - 0.8, 0.03) * (0.25 + 0.75 * np.exp(-((r - 0.76) / 0.05) ** 2)), 0, 1)
    # Cracks: random walks that branch.
    im, d, s = stamp_canvas(2)
    r_ = np.random.default_rng(15)
    def walk(px, py, ang, steps, wd):
        for _ in range(steps):
            ang += r_.normal(0, 0.35)
            nx, ny = px + math.cos(ang) * 7 * s, py + math.sin(ang) * 7 * s
            d.line([px, py, nx, ny], fill=255, width=max(1, int(wd * s)))
            px, py = nx, ny
            wd = max(1.0, wd * 0.985)
            if r_.random() < 0.05 and steps > 10:
                walk(px, py, ang + r_.choice([-1, 1]) * r_.uniform(0.6, 1.2), int(steps * 0.5), wd * 0.7)
    for k in range(5):
        walk(256 * s, 256 * s, k * 2 * math.pi / 5 + r_.normal(0, 0.3), 40, 6)
    out['Cracks'] = falloff(np.asarray(down(im)).astype(np.float32) / 255.0, 0.15)
    # Scratches: thin straight-ish strokes.
    for name, cnt, seed in [('Scratches', 40, 16), ('Scratches Dense', 160, 17)]:
        im, d, s = stamp_canvas(2)
        r_ = np.random.default_rng(seed)
        for _ in range(cnt):
            px, py = r_.uniform(60, 452) * s, r_.uniform(60, 452) * s
            ang = r_.normal(0.4, 0.5)
            ln = r_.uniform(40, 220) * s
            d.line([px, py, px + math.cos(ang) * ln, py + math.sin(ang) * ln], fill=int(r_.uniform(120, 255)), width=max(1, int(r_.uniform(1, 3) * s)))
        out[name] = falloff(np.asarray(down(im)).astype(np.float32) / 255.0, 0.2)
    # Drips and leaks: streaks running down.
    # A spill along the top with a ragged lower edge; drips end in a bead,
    # leaks thin out and fade as they run.
    for name, cnt, seed in [('Drips', 11, 18), ('Leaks', 30, 19)]:
        im, d, s = stamp_canvas(2)
        r_ = np.random.default_rng(seed)
        xs = np.linspace(60, 452, 50)
        edge = 95 + np.cumsum(r_.normal(0, 4, xs.size))
        edge = edge - (edge.mean() - 95)
        top = 55 + r_.normal(0, 3, xs.size)
        taper = np.clip(np.minimum(xs - 60, 452 - xs) / 60, 0, 1)
        lo, hi = 75 + (edge - 75) * taper, 75 - (75 - top) * taper
        d.polygon([(x * s, y * s) for x, y in zip(xs, hi)] + [(x * s, y * s) for x, y in zip(xs[::-1], lo[::-1])], fill=255)
        for _ in range(cnt):
            px = r_.uniform(90, 420)
            y0 = float(np.interp(px, xs, lo)) - 4
            ln = r_.uniform(60, 400) * (0.6 if name == 'Leaks' else 1.0)
            wd = r_.uniform(8, 22) if name == 'Drips' else r_.uniform(3, 12)
            steps = 24
            for k in range(steps):
                t0, t1 = k / steps, (k + 1) / steps
                w0 = wd * (1 - 0.45 * t0)
                fade = 1.0 if name == 'Drips' else (1 - t0) ** 0.8
                wob = math.sin(px * 0.1 + t0 * 3) * 1.5
                d.polygon([((px - w0 / 2 + wob) * s, (y0 + ln * t0) * s), ((px + w0 / 2 + wob) * s, (y0 + ln * t0) * s),
                           ((px + w0 / 2 * 0.98 + wob) * s, (y0 + ln * t1 + 1) * s), ((px - w0 / 2 * 0.98 + wob) * s, (y0 + ln * t1 + 1) * s)],
                          fill=int(255 * fade))
            if name == 'Drips':
                b = wd * 0.75
                d.ellipse([(px - b) * s, (y0 + ln - b) * s, (px + b) * s, (y0 + ln + b * 1.2) * s], fill=255)
        a = np.asarray(down(im.filter(ImageFilter.GaussianBlur(1.5 * s)))).astype(np.float32) / 255.0
        out[name] = a if name == 'Drips' else a * (0.6 + 0.4 * fbm(A, 1.5, seed=20))
    out['Rust Spots'] = falloff(levels(fbm(A, 1.9, seed=21) * fbm(A, 2.5, seed=22) * 2.2, 0.32, 0.5), 0.25)
    out['Dust'] = falloff(levels(blur((rng.random((A, A)) < 0.06).astype(np.float32), 1.0) * fbm(A, 1.8, seed=23) * 4, 0.05, 0.35), 0.3)
    out['Chipped Paint'] = falloff(levels(worley(A, 9, seed=24) + 0.35 * fbm(A, 2.2, seed=25), 0.62, 0.68), 0.2)
    out['Fibers'] = falloff(np.clip(levels(blur(np.abs(np.sin((x * 30 + 3 * fbm(A, 1.2, seed=26)) * 3.0)) ** 12, 0.6), 0.05, 0.6) * (0.5 + fbm(A, 1.0, seed=27)), 0, 1), 0.2)
    out['Ink Blot'] = levels(splatter(28, blobs=40, spread=0.35, size=(0.04, 0.14), droplets=40), 0.5, 0.55)
    out['Cloud'] = np.clip(fbm(A, 2.2, seed=29) * 1.6 - 0.3, 0, 1) * np.exp(-(r / 0.7) ** 4)
    for name, img in out.items():
        save(img, 'alphas', 'Grunge', name)


# ------------------------------------------------------------ alphas: brush tips
def brush_tips():
    x, y, r, t = polar()
    out = {}
    out['Airbrush'] = np.exp(-(r / 0.42) ** 2)
    out['Hard Round'] = smooth(r - 0.95, 2.0 / A)
    out['Soft Round'] = np.clip(1 - r, 0, 1) ** 2
    out['Chalk'] = smooth(r - 0.9, 0.08) * levels(fbm(A, 0.9, seed=31) + 0.25 * (rng.random((A, A)) - 0.5), 0.36, 0.55)
    out['Charcoal'] = smooth(np.maximum(np.abs(x) * 0.7, np.abs(y) * 1.3) - 0.85, 0.05) * levels(fbm(A, 0.6, seed=32), 0.4, 0.62)
    out['Pencil'] = smooth(r - 0.9, 0.05) * levels(rng.random((A, A)) * 0.5 + 0.5 * fbm(A, 0.5, seed=33), 0.35, 0.7)
    out['Marker'] = smooth(np.maximum(np.abs(x), np.abs(y) * 2.2) - 0.85, 0.06) * 0.92
    out['Sponge'] = smooth(r - 0.9, 0.1) * levels(worley(A, 14, seed=34), 0.25, 0.4)
    bristles = np.zeros((A, A), np.float32)
    r_ = np.random.default_rng(35)
    for _ in range(160):
        cx, cy = r_.normal(0, 0.4), r_.normal(0, 0.16)
        rad = r_.uniform(0.03, 0.08)
        bristles = np.maximum(bristles, r_.uniform(0.5, 1.0) * np.exp(-(((x - cx) / rad) ** 2 + ((y - cy) / (rad * 0.9)) ** 2)))
    out['Bristles'] = levels(bristles, 0.15, 0.9)
    out['Dry Brush'] = levels(blur(np.abs(np.sin(y * 60 + 4 * fbm(A, 1.0, seed=36))) ** 6, 0.6) * smooth(np.maximum(np.abs(x), np.abs(y) * 1.2) - 0.85, 0.12) * fbm(A, 0.8, seed=37) * 2, 0.2, 0.8)
    out['Watercolor'] = (0.35 + 0.65 * np.exp(-((r * (1 + 0.12 * (fbm(A, 2.0, seed=38) - 0.5)) - 0.78) / 0.08) ** 2)) * smooth(r * (1 + 0.12 * (fbm(A, 2.0, seed=38) - 0.5)) - 0.82, 0.03)
    out['Square Soft'] = smooth(np.maximum(np.abs(x), np.abs(y)) - 0.8, 0.18)
    out['Line'] = smooth(np.maximum(np.abs(x) - 0.92, np.abs(y) - 0.05), 2.0 / A)
    out['Fan'] = smooth(r - 0.92, 0.04) * smooth(np.abs(t + math.pi / 2) - 0.7, 0.05) * (0.4 + 0.6 * np.abs(np.sin(t * 40)) ** 0.3)
    for name, img in out.items():
        save(img, 'alphas', 'Brush tips', name)


# ------------------------------------------------------------ alphas: symbols and text (Hack font, MIT / Bitstream Vera license)
def symbols(font_path):
    w = 2.0 / A
    x, y, r, t = polar()
    out = {}
    tri = Image.new('L', (A * 4, A * 4), 0)
    d = ImageDraw.Draw(tri)
    s = 4
    d.polygon(poly(256 * s, 300 * s, 250 * s, 3, -math.pi / 2), fill=255)
    d.polygon(poly(256 * s, 300 * s, 190 * s, 3, -math.pi / 2), fill=0)
    d.rounded_rectangle([238 * s, 190 * s, 274 * s, 340 * s], radius=14 * s, fill=255)
    d.ellipse([234 * s, 360 * s, 278 * s, 404 * s], fill=255)
    save(down(tri), 'alphas', 'Symbols', 'Warning')
    out['Prohibited'] = np.maximum(smooth(np.abs(r - 0.82) - 0.1, w), smooth(np.abs((x + y) / 1.414) - 0.09, w) * smooth(r - 0.8, w))
    out['Target'] = np.maximum.reduce([smooth(np.abs(r - k) - 0.05, w) for k in (0.85, 0.55, 0.25)] + [smooth(r - 0.08, w)])
    out['Crosshair'] = np.maximum(smooth(np.abs(r - 0.7) - 0.04, w), smooth(np.minimum(np.maximum(np.abs(x) - 0.95, np.abs(y) - 0.03), np.maximum(np.abs(y) - 0.95, np.abs(x) - 0.03)), w) * (1 - smooth(r - 0.25, w)))
    gap = np.abs(np.arctan2(x, -y)) < 0.5
    out['Power'] = np.maximum(smooth(np.abs(r - 0.72) - 0.08, w) * (~gap).astype(np.float32), smooth(np.maximum(np.abs(x) - 0.08, np.abs(y + 0.3) - 0.42), w))
    out['Medical Cross'] = smooth(np.minimum(np.maximum(np.abs(x) - 0.25, np.abs(y) - 0.8), np.maximum(np.abs(y) - 0.25, np.abs(x) - 0.8)), w)
    out['Hazard Stripes'] = smooth(np.abs(((x + y) * 2.5) % 1 - 0.5) - 0.25, 0.01) * smooth(np.maximum(np.abs(x), np.abs(y)) - 0.9, w)
    for name, img in out.items():
        save(img, 'alphas', 'Symbols', name)
    if font_path and os.path.exists(font_path):
        for word in ['CAUTION', 'DANGER', 'WARNING', 'STOP', 'EXIT', 'FRAGILE']:
            im = Image.new('L', (A * 2, A * 2), 0)
            d = ImageDraw.Draw(im)
            size = 400
            f = ImageFont.truetype(font_path, size)
            while d.textlength(word, font=f) > A * 1.8 and size > 40:
                size -= 10
                f = ImageFont.truetype(font_path, size)
            bb = d.textbbox((0, 0), word, font=f)
            d.text(((A * 2 - (bb[2] - bb[0])) / 2 - bb[0], (A * 2 - (bb[3] - bb[1])) / 2 - bb[1]), word, fill=255, font=f)
            save(down(im), 'alphas', 'Symbols', 'Text ' + word.capitalize())
        for k in range(10):
            im = Image.new('L', (A * 2, A * 2), 0)
            d = ImageDraw.Draw(im)
            f = ImageFont.truetype(font_path, 820)
            bb = d.textbbox((0, 0), str(k), font=f)
            d.text(((A * 2 - (bb[2] - bb[0])) / 2 - bb[0], (A * 2 - (bb[3] - bb[1])) / 2 - bb[1]), str(k), fill=255, font=f)
            save(down(im), 'alphas', 'Symbols', 'Number %d' % k)


# ------------------------------------------------------------ alphas: patterns
def pattern_alphas():
    x, y, r, t = polar()
    w = 2.0 / A
    out = {}
    hx = x * 4
    hy = y * 4
    out['Checker'] = (((np.floor(x * 4) + np.floor(y * 4)) % 2) > 0).astype(np.float32)
    out['Stripes'] = smooth(np.abs((x * 4) % 1 - 0.5) - 0.25, 0.01)
    out['Grid'] = 1 - smooth(np.maximum(np.abs((x * 4) % 1 - 0.5), np.abs((y * 4) % 1 - 0.5)) - 0.44, 0.01)
    out['Halftone'] = smooth(np.sqrt(((x * 6) % 1 - 0.5) ** 2 + ((y * 6) % 1 - 0.5) ** 2) - 0.45 * (1 - (r / 1.4)), 0.02)
    # Hexagon cells.
    q = (hx * 2 / 3)
    out['Honeycomb'] = 1 - smooth(np.abs(hexd(x * 3 + 10, y * 3 + 10) - 0.5) - 0.05, 0.01)
    out['Scales'] = smooth(np.sqrt(((x * 5) % 1 - 0.5) ** 2 + (((y * 5 + 0.5 * (np.floor(x * 5) % 2)) % 1) - 0.0) ** 2) - 0.5, 0.02)
    for name, img in out.items():
        save(img * smooth(np.maximum(np.abs(x), np.abs(y)) - 0.98, w), 'alphas', 'Patterns', name)


# ------------------------------------------------------------ textures (tileable)
def textures():
    n = T
    y, x = np.mgrid[0:n, 0:n].astype(np.float32) / n
    g = {}
    # Grunges.
    g['Clouds'] = fbm(n, 2.4, seed=101)
    g['Clouds Soft'] = fbm(n, 3.0, seed=102)
    g['Noise Fine'] = fbm(n, 0.6, seed=103)
    g['Noise Grain'] = np.clip(0.5 + 0.25 * np.random.default_rng(104).standard_normal((n, n)), 0, 1)
    g['Grunge Dirt'] = levels(fbm(n, 2.0, seed=105) * 0.7 + fbm(n, 1.0, seed=106) * 0.3, 0.4, 0.75)
    g['Grunge Spots'] = levels(fbm(n, 2.4, seed=107) * fbm(n, 1.4, seed=108) * 2.1, 0.3, 0.55)
    g['Grunge Rust'] = levels(fbm(n, 1.8, seed=109) + 0.3 * worley(n, 18, seed=110), 0.45, 0.8)
    g['Grunge Concrete'] = levels(fbm(n, 1.5, seed=111) * 0.6 + 0.4 * (np.random.default_rng(112).random((n, n)) < 0.03).astype(np.float32), 0.25, 0.8)
    g['Grunge Paint Peel'] = levels(worley(n, 12, seed=113) + 0.45 * fbm(n, 2.2, seed=114), 0.62, 0.66)
    g['Grunge Edge Damage'] = levels(fbm(n, 1.3, seed=115) * (0.5 + 0.5 * fbm(n, 2.6, seed=116)) * 2, 0.45, 0.7)
    g['Grunge Smudges'] = levels(blur(fbm(n, 2.0, seed=117), 8) * 1.4, 0.5, 0.75)
    g['Grunge Water Stains'] = np.clip(np.exp(-((fbm(n, 2.6, seed=118) - 0.55) / 0.02) ** 2) * 0.8 + levels(fbm(n, 2.6, seed=118), 0.55, 0.7) * 0.35, 0, 1)
    streak = blur(np.random.default_rng(119).random((n, n)), 1.0)
    streak = np.real(np.fft.ifft2(np.fft.fft2(streak) * np.exp(-2 * (np.pi * 60) ** 2 * (np.fft.fftfreq(n)[:, None] ** 2))))
    g['Grunge Leaks'] = levels((streak - streak.mean()) / (streak.std() + 1e-9) * 0.2 + 0.5, 0.45, 0.8) * (0.6 + 0.4 * fbm(n, 1.8, seed=120))
    scr = np.zeros((n, n), np.float32)
    im = Image.new('L', (n, n), 0)
    d = ImageDraw.Draw(im)
    r_ = np.random.default_rng(121)
    for _ in range(600):
        px, py = r_.uniform(0, n, 2)
        ang = r_.normal(0.3, 0.4)
        ln = r_.uniform(20, 180)
        for ox in (-n, 0, n):
            for oy in (-n, 0, n):
                d.line([px + ox, py + oy, px + ox + math.cos(ang) * ln, py + oy + math.sin(ang) * ln], fill=int(r_.uniform(80, 255)), width=1)
    g['Grunge Scratches'] = np.asarray(im).astype(np.float32) / 255.0
    g['Cells'] = np.clip(worley(n, 10, seed=122), 0, 1)
    g['Cell Edges'] = np.clip(1 - worley(n, 10, seed=123, f2=True) * 3, 0, 1)
    g['Cracked Ground'] = levels(1 - worley(n, 14, seed=124, f2=True) * 6 - 0.3 * (fbm(n, 2.0, seed=125) - 0.5), 0.75, 0.95)
    g['Dust Particles'] = levels(blur((np.random.default_rng(126).random((n, n)) < 0.003).astype(np.float32), 1.2) * 12, 0.05, 0.6)
    for name, img in g.items():
        save(img, 'textures', 'Grunges', name)
    # Patterns (tileable by construction).
    p = {}
    k = 8
    p['Checker'] = (((np.floor(x * k) + np.floor(y * k)) % 2)).astype(np.float32)
    p['Stripes'] = smooth(np.abs((x * k) % 1 - 0.5) - 0.25, 0.01)
    p['Stripes Diagonal'] = smooth(np.abs(((x + y) * k) % 1 - 0.5) - 0.25, 0.01)
    weave_u = 0.5 + 0.5 * np.cos(2 * np.pi * (y * 32 % 1 - 0.5)) * (((np.floor(x * 32) + np.floor(y * 32)) % 2) * 2 - 1)
    p['Fabric Weave'] = np.clip(weave_u * 0.8 + 0.2 * fbm(n, 0.8, seed=131), 0, 1)
    twill = ((np.floor(x * 64) + np.floor(y * 64)) % 4 < 2).astype(np.float32)
    p['Denim Twill'] = np.clip(blur(twill, 0.7) * 0.75 + 0.25 * fbm(n, 0.8, seed=132), 0, 1)
    hb = ((np.floor(x * 16) % 2) * 2 - 1)
    p['Herringbone'] = smooth(np.abs(((y * 16 + hb * x * 16) % 1) - 0.5) - 0.22, 0.02)
    hdt = (((np.floor(x * 16) + np.floor(y * 16)) % 2 == 0) | ((np.floor((x + y) * 16 * 0.999) % 4) == 0)).astype(np.float32)
    p['Steps Check'] = hdt
    p['Brick'] = 1 - smooth(np.maximum(np.abs((y * 8) % 1 - 0.5) * 2, np.abs(((x * 4 + 0.5 * (np.floor(y * 8) % 2)) % 1) - 0.5) * 2) - 0.9, 0.01)
    p['Tiles Square'] = 1 - smooth(np.maximum(np.abs((x * 6) % 1 - 0.5), np.abs((y * 6) % 1 - 0.5)) - 0.47, 0.005)
    # 8 cells across, 5 rows of pairs down: tiles (a hair squashed vertically).
    p['Hexagons'] = 1 - smooth(np.abs(hexd(x * 8, y * 5 * math.sqrt(3.0)) - 0.5) - 0.04, 0.008)
    p['Scales'] = smooth(np.sqrt(((x * 12) % 1 - 0.5) ** 2 + ((y * 12 + 0.5 * (np.floor(x * 12) % 2)) % 1) ** 2) - 0.55, 0.02)
    p['Perforated'] = smooth(np.sqrt(((x * 16) % 1 - 0.5) ** 2 + ((y * 16) % 1 - 0.5) ** 2) - 0.3, 0.02)
    p['Carbon Fiber'] = np.clip(0.5 + 0.5 * np.cos(2 * np.pi * (x * 24 + y * 24)) * (((np.floor(x * 24) + np.floor(y * 24)) % 2) * 2 - 1) * 0.8, 0, 1)
    dp = np.abs(((x * 6 + y * 6) % 1) - 0.5) * 2
    dp2 = np.abs(((x * 6 - y * 6 + 0.5) % 1) - 0.5) * 2
    p['Diamond Plate'] = np.maximum(np.exp(-((dp - 0.5) / 0.08) ** 2) * (((np.floor(x * 6) + np.floor(y * 6)) % 2) == 0), np.exp(-((dp2 - 0.5) / 0.08) ** 2) * (((np.floor(x * 6) + np.floor(y * 6)) % 2) == 1))
    p['Knurling'] = 0.5 + 0.25 * np.cos(2 * np.pi * (x + y) * 40) + 0.25 * np.cos(2 * np.pi * (x - y) * 40)
    p['Grid Lines'] = 1 - smooth(np.maximum(np.abs((x * 8) % 1 - 0.5), np.abs((y * 8) % 1 - 0.5)) - 0.48, 0.004)
    for name, img in p.items():
        save(img, 'textures', 'Patterns', name)
    # Stamps (heights for normal / height fills; white is high), centered.
    s = {}
    X, Y = (x - 0.5) * 2, (y - 0.5) * 2
    R = np.sqrt(X * X + Y * Y)
    bevel = lambda d, b: np.clip(-d / b, 0, 1)
    s['Button Round'] = bevel(R - 0.8, 0.15)
    s['Button Domed'] = np.sqrt(np.clip(1 - (R / 0.8) ** 2, 0, 1))
    sq = np.maximum(np.abs(X), np.abs(Y))
    s['Panel Square'] = bevel(sq - 0.85, 0.12)
    rr = np.sqrt(np.maximum(np.abs(X) - 0.5, 0) ** 2 + np.maximum(np.abs(Y) - 0.3, 0) ** 2)
    s['Panel Rounded'] = bevel(rr - 0.3, 0.12)
    s['Niche Round'] = 1 - bevel(R - 0.7, 0.12)
    s['Niche Square'] = 1 - bevel(sq - 0.7, 0.1)
    s['Ring Embossed'] = np.exp(-((R - 0.6) / 0.08) ** 2)
    slots = np.maximum.reduce([bevel(np.sqrt(np.maximum(np.abs(X) - 0.6, 0) ** 2 + (Y - c) ** 2) - 0.06, 0.04) for c in np.linspace(-0.6, 0.6, 6)])
    s['Vent Slots'] = 1.0 - slots
    s['Grill Holes'] = np.clip(smooth(np.sqrt(((X * 4) % 1 - 0.5) ** 2 + ((Y * 4) % 1 - 0.5) ** 2) - 0.3, 0.05) * smooth(R - 0.9, 0.02) + smooth(-(R - 0.9), 0.02), 0, 1)
    s['Handle Bar'] = np.exp(-(Y / 0.18) ** 2) * bevel(np.abs(X) - 0.75, 0.15)
    hexb = np.maximum(np.abs(X) * 0.866 + np.abs(Y) * 0.5, np.abs(Y))
    s['Hex Bolt'] = bevel(hexb - 0.6, 0.08) * 0.8 + 0.2 * bevel(R - 0.4, 0.05)
    s['Rivet'] = np.sqrt(np.clip(1 - (R / 0.35) ** 2, 0, 1))
    s['Cross Bevel'] = bevel(np.minimum(np.maximum(np.abs(X) - 0.2, np.abs(Y) - 0.75), np.maximum(np.abs(Y) - 0.2, np.abs(X) - 0.75)), 0.1)
    s['Gear Emboss'] = bevel(R - (0.65 + 0.08 * np.sign(np.cos(np.arctan2(Y, X) * 12))), 0.06) * (1 - bevel(R - 0.25, 0.05))
    for name, img in s.items():
        save(img, 'textures', 'Stamps', name)


def contact_sheet(kind, path, cell=96):
    files = []
    root = os.path.join(OUT, kind)
    for cat in sorted(os.listdir(root)):
        for f in sorted(os.listdir(os.path.join(root, cat))):
            files.append((cat, f, os.path.join(root, cat, f)))
    cols = 16
    rows = (len(files) + cols - 1) // cols
    sheet = Image.new('L', (cols * cell, rows * (cell + 14)), 30)
    d = ImageDraw.Draw(sheet)
    for i, (cat, f, p) in enumerate(files):
        im = Image.open(p).convert('L').resize((cell - 4, cell - 4), Image.LANCZOS)
        cx, cy = (i % cols) * cell, (i // cols) * (cell + 14)
        sheet.paste(im, (cx + 2, cy + 2))
        d.text((cx + 2, cy + cell), f[:-4][:15], fill=200)
    sheet.save(path)
    return len(files)


if __name__ == '__main__':
    font = sys.argv[2] if len(sys.argv) > 2 else None
    shapes()
    stitches()
    grunge_alphas()
    brush_tips()
    symbols(font)
    pattern_alphas()
    textures()
    sd = sys.argv[3] if len(sys.argv) > 3 else '.'
    print('alphas', contact_sheet('alphas', os.path.join(sd, 'alphas_sheet.png')))
    print('textures', contact_sheet('textures', os.path.join(sd, 'textures_sheet.png'), 120))
