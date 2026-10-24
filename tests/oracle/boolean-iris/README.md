# Inductive iris by Boolean differences

The iris of the `iris-inductive` case, built not from waveguide sections but from a 32 mm box of
WR-90 with two plates subtracted (`geometry.feature`, `combine: subtract`): each plate runs the
full height, flush with both broad walls and with its side wall, and the mesh is unstructured.
The flush faces are moved past the guide's before the surfaces are intersected, so the
difference meets in general position. The result is held to the same mode-matching reference:
the Boolean surface, the unstructured mesh and the solver together reproduce the iris to the
accuracy uniform elements allow at its edges.
