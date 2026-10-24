# H-plane step imported from STL

The asymmetric H-plane step of `hplane-step-offset` — WR-90 for 15 mm, then 16 mm wide against
the wall x = 0 for 18 mm — drawn as a closed 20-triangle surface in ASCII STL (`step.stl`, in
millimetres, written by a short script from the corners) and read with `geometry.import`. The
importer groups the triangles into the body's faces by the angle between neighbours, the mesher
recognises the rectangles on the lowest and highest z planes as ports, and the result is held to
the same mode-matching reference as the section-built step.
