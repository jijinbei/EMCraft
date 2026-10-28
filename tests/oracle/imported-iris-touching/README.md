# Inductive iris imported as bodies touching the walls

The iris of `iris-inductive` and `boolean-iris` as three SolidCraft bodies (`iris.3mf`, exported
with face tags by the SolidCraft CLI from `iris.solidcraft.json`): the WR-90 guide `Guide`
(22.86 × 10.16 × 32 mm) and the plates `IrisL` and `IrisR` (5.43 × 10.16 × 2 mm at z = 15 mm),
each against a side wall and both broad walls. `geometry.import` with `region: Guide` makes the
guide the region and the plates metal.

This is the regression case for plates touching the walls: each plate was moved by a millionth of
the element size off the walls, leaving slivers of air a nanometre thick, and the mesh held flat
tetrahedra the solver refused. Now an imported body's faces flush with the region's are moved out
of it before the Boolean (as primitive features' are), the points the Boolean leaves along the
straight creases are collapsed, and every mesh is checked for degenerate tetrahedra where it is
made. The result is held to the same mode-matching reference as `boolean-iris`.
