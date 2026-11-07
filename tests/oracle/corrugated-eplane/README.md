# E-plane corrugated guide

WR-90 whose narrow wall waves sinusoidally, b(z) = 10.16 + 2 sin(2π(z − 10)/10) mm, for four
periods between two straight leads 10 mm long (60 mm in all): the `corrugated-guide`
template's guide. A SolidCraft design compiled into EMCraft
(`crates/templates/designs/corrugated-e.solidcraft`, written by `make-corrugated.rs.txt`), its
waving side an equation curve; imported as the region, meshed with 2 mm tetrahedra, solved at
order 2.

The reference is independent of the finite elements: the wall cut into steps in height,
solved by mode matching (`emcraft-theory`), its limit as the steps shrink extrapolated from
160, 320 and 640 steps. The two stand about 0.3° apart (152.70° and 152.52°): the finite
elements' curved face is faceted, and a step in height carries a field singular at its edge,
so neither is exact to better than about 0.2°.

What it shows: TE10's propagation constant does not depend on the narrow wall's height, so the
slow-variation estimate ∫β dz predicts no change at all; yet S21 lags a straight guide's
−βL = −543.98° by 23.3°. The corrugation loads the guide with the evanescent modes it stirs
up — reactively, as a capacitive iris does — and slows the wave: the delay grows from 13° at
8.2 GHz to 47° at 12.4 GHz, towards the Bragg band where 2β = 2π/p (about 16 GHz). A broad
wall waving the same way advances the phase instead (`corrugated-hplane`).
