# Inductive post, time domain on a solid

The `adaptive-post` geometry — a 3 mm conducting post across WR-90, cut from the box by a
Boolean difference — solved by the time-domain solver for solids on 0.5 mm cells: the post
staircased (cells whose centres lie in it, and edges whose midpoints do, are metal). An
independent discretisation of the problem the FEM case checks, against the same integral-equation
reference; the staircase is what its tolerance allows for.
