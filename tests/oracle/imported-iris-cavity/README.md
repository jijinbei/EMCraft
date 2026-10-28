# Inductive iris imported as its metal (cavity)

The iris of `iris-inductive` and `boolean-iris`, drawn the way a CAD user draws a waveguide
component: the metal. `block.3mf` is one SolidCraft body — a 32.86 × 20.16 × 32 mm block, the
22.86 × 10.16 mm WR-90 channel cut through it along z and two 5.43 × 10.16 × 2 mm plates joined
to the channel's side walls at z = 15 mm — exported with face tags by the SolidCraft CLI from
`block.solidcraft.json` (`solidcraft-cli run block.solidcraft.json`).

`geometry.import` with `region: cavity` makes the region the air the metal encloses: the
block's bounding box less the block, of whose connected pieces those that reach no side (x or y)
of the box are kept — here the channel, open at both z ends, where the ports go. The block's
outer faces lie in the box's, so they are moved out of the air before the Boolean, and the points
the Boolean leaves along the straight creases are collapsed. The result is held to the same
mode-matching reference as `boolean-iris`; the native recipe, the same iris cut in SolidCraft
(one body as the region) and the iris plates imported as bodies in the guide (`imported-iris-touching`)
agree with it within 4e-3 in |S11| (0.7764 here, 0.7731 native, 0.7761 cut, 0.7747 touching, at 10 GHz).
