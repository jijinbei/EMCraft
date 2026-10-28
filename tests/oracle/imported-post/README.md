# Inductive post imported from SolidCraft

The `adaptive-post` geometry — a round conducting post, 3 mm across, from one broad wall of
WR-90 to the other, centred in 30 mm of guide — drawn in SolidCraft instead of with EMCraft's
primitives: a box with a cylinder cut from it, exported as 3MF with B-rep face tags
(`post.3mf`, made by `solidcraft-cli run solidcraft.json`). The file holds only triangles: the
post's side comes as 26 flat facets round (SolidCraft's chord tolerance, 2e-4 of the body's
size), split at its seam into two tagged faces.

`geometry.import` fits each tagged face's vertices with a plane, a cylinder, a sphere and a
cone (`emcraft-io::fit`: algebraic starts refined by Levenberg–Marquardt on geometric distances,
after Lukács, Martin and Marshall, 1998) and keeps a fit when every vertex lies on it to 1e-6 of
the body's size and the triangles hug it as chords; the reply counts two cylinders and six
planes. The mesher then puts the points it adds on the post onto the fitted cylinder, and the
second-order elements bend their edges onto it, exactly as for the native `geometry.feature`
cylinder of `adaptive-post`. The recipe and the reference are that case's.

Measured (|S11 − reference|, phase error), the same adaptive loop:

| surface | pass | unknowns | |S11| error | phase error |
|---|---|---|---|---|
| faceted (before fitting) | 2 | — | 5.1e-4 | −0.17° |
| fitted cylinder | 1 | 127 748 | −1.3e-5 | +0.044° |
| fitted cylinder | 2 | 133 978 | +5.6e-5 | +0.020° |
| fitted cylinder | 3 | 181 064 | +3.5e-5 | +0.011° |
| fitted cylinder | 4 | 220 696 | +6.0e-6 | +0.004° |
| native cylinder (`adaptive-post`) | 3 | 103 528 | +1.4e-5 | +0.007° |

The faceted post's error is its geometry: S moves by 478 per metre of radius, and the facets'
mean radius is about 1 µm short of the circle. With the cylinder recovered, the passes converge
on the reference as the native post's do. The first mesh is finer than the native case's (the
broad walls arrive as 646 triangles each, fanned round the hole), so the loop stops one pass
earlier; the case stops there (ΔS < 3e-3) to stay quick.
