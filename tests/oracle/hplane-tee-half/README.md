# H-plane tee on its half

The [hplane-tee](../hplane-tee/) junction cut by a symmetry plane at mid-height (z = b/2) closed
by an electric wall (`model.symmetry` axis z, kind electric): TE10's E is normal to the plane,
so it and every mode the tee couples it to are kept, and only the lower half is meshed and
solved. Its S-parameters are held to the same field-superposition reference and tolerances as
the whole tee's, on about half the unknowns.
