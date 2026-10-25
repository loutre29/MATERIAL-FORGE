# Starter alphas: originality check (2026-10-24)

Question: are any of the starter alphas (`starter_assets/alphas`, 116
images) copies of Substance 3D Painter's alphas?

## Provenance

Every starter alpha is drawn by `tools/gen_starter_assets.py` from code:
analytic shapes (signed distances, polygons), seeded noise (fBm, Worley),
PIL drawing, and the Hack font (MIT / Bitstream Vera license) for words and
numbers. The script reads no image file. No Painter file was opened, read
or decoded to make them.

## Measurement

`tools/compare_alphas.py <repo> <report.txt>` compares each starter alpha
with the 654 tiles of the user's two shelf screenshots (`alphas 01.png`,
`alphas 02.png`, Painter's own alphas as shown in its UI):

1. Normalized cross-correlation of the gray values at 52 px, best over
   small offsets. 1.0 means the same image.
2. For the 30 closest pairs, the correlation of their gradient magnitude
   (edges and grain), which tells a shared silhouette from shared detail.

Results:

| Measure | Value |
|---|---|
| Gray correlation, median over the 116 alphas | 0.76 |
| Gray correlation, maximum | 0.99 (Circle Soft vs Painter's "Shape", both a radial falloff) |
| Edge correlation, maximum of the 30 closest | 0.73 (Seam Ridge vs a soft horizontal line) |

Every pair above 0.9 is a primitive that any paint program ships: soft
round falloffs, a hard disc, a soft square, a plus, a diamond, a bar, a
line, a dome. None of the grunge, splatter, stitch, hardware, pattern or
symbol alphas has a close match: their best gray correlations are those of
any two blobs, and their edges do not line up.

Conclusion: no starter alpha copies a Painter alpha. Redo this check after
changing the generator (`python tools/compare_alphas.py . report.txt`).
