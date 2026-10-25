"""Compares our starter alphas with the tiles of the user's Painter shelf
screenshots: normalized cross-correlation, best over small offsets."""
import sys, glob, os
import numpy as np
from PIL import Image

repo = sys.argv[1]
out = sys.argv[2]
S = 52  # compared size

def tiles(path, cols, rows):
    im = np.asarray(Image.open(path).convert('L')).astype(np.float32)
    h, w = im.shape
    pw, ph = w / cols, h / rows
    res = []
    for r in range(rows):
        for c in range(cols):
            x0, y0 = int(c * pw + 4), int(r * ph + 4)
            t = im[y0:y0 + int(pw) - 8, x0:x0 + int(pw) - 8]
            if t.std() < 3:
                continue
            res.append(((os.path.basename(path), r, c), t))
    return res

def norm(a):
    a = a - a.mean()
    n = np.sqrt((a * a).sum())
    return a / n if n > 0 else a

refs = []
for p, cols, rows in [('alphas 01.png', 22, 15), ('alphas 02.png', 22, 15)]:
    for key, t in tiles(os.path.join(repo, p), cols, rows):
        img = Image.fromarray(t.astype(np.uint8)).resize((S + 8, S + 8), Image.BILINEAR)
        refs.append((key, np.asarray(img).astype(np.float32), t))

ours = sorted(glob.glob(os.path.join(repo, 'starter_assets', 'alphas', '*', '*.png')))
rows = []
for f in ours:
    a = np.asarray(Image.open(f).convert('L').resize((S, S), Image.BILINEAR)).astype(np.float32)
    na = norm(a)
    best = (-1, None)
    for key, r, _ in refs:
        for dy in range(0, 9, 2):
            for dx in range(0, 9, 2):
                v = float((na * norm(r[dy:dy + S, dx:dx + S])).sum())
                if v > best[0]:
                    best = (v, key)
    rows.append((best[0], os.path.relpath(f, os.path.join(repo, 'starter_assets', 'alphas')), best[1]))
rows.sort(reverse=True)
with open(out, 'w', encoding='utf-8') as fh:
    for v, f, k in rows:
        fh.write(f'{v:.3f}  {f}  ~ {k}\n')
vals = np.array([r[0] for r in rows])
print('alphas', len(rows), 'reference tiles', len(refs))
print('max %.3f  median %.3f  >0.9: %d  >0.8: %d' % (vals.max(), np.median(vals), (vals > 0.9).sum(), (vals > 0.8).sum()))
for v, f, k in rows[:15]:
    print(f'{v:.3f}  {f}  ~ {k}')

# Detail check: correlation of the gradient magnitude (edges, grain).
def grad(a):
    gy, gx = np.gradient(a)
    return np.hypot(gx, gy)

refmap = {k: t for k, _, t in refs}
sheet = Image.new('L', (2 * 70 * 3, 70 * 10), 30)
detail = []
for i, (v, f, k) in enumerate(rows[:30]):
    t = refmap[k]
    n = t.shape[0]
    a = np.asarray(Image.open(os.path.join(repo, 'starter_assets', 'alphas', f)).convert('L').resize((n, n), Image.BILINEAR)).astype(np.float32)
    g = float((norm(grad(a)) * norm(grad(t[:n, :n]))).sum())
    detail.append((g, v, f, k))
    col, row = i // 10, i % 10
    sheet.paste(Image.fromarray(a.astype(np.uint8)).resize((64, 64)), (col * 140 + 3, row * 70 + 3))
    sheet.paste(Image.fromarray(t.astype(np.uint8)).resize((64, 64)), (col * 140 + 70, row * 70 + 3))
sheet.save(os.path.join(os.path.dirname(out), 'alpha_compare_sheet.png'))
print('edge correlation of the 30 closest (1.0 = same edges):')
for g, v, f, k in sorted(detail, reverse=True)[:12]:
    print(f'  edges {g:.2f}  ncc {v:.2f}  {f}')
