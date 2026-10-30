# Literature

The methods EMCraft implements and where they come from.

## Meshing
The tetrahedral mesher (`emcraft-tetmesh`) is FieldCraft's; its modules cite their sources. In
short: J. R. Shewchuk, *Adaptive precision floating-point arithmetic and fast robust geometric
predicates*, Discrete Comput. Geom. 18(3):305–363, 1997; A. Bowyer, *Computing Dirichlet
tessellations*, and D. F. Watson, *Computing the n-dimensional Delaunay tessellation*, Comput.
J. 24(2), 1981; L. Diazzi, D. Panozzo, A. Vaxman, M. Attene, *Constrained Delaunay
tetrahedrization: a robust and practical approach*, ACM TOG 42(6), 2023; J. Tournois, C.
Wormser, P. Alliez, M. Desbrun, *Interleaving Delaunay refinement and optimization for practical
isotropic tetrahedron mesh generation*, ACM TOG 28(3), 2009; B. M. Klingner, J. R. Shewchuk,
*Aggressive tetrahedral mesh improvement*, Proc. 16th International Meshing Roundtable, 2007.
- S. W. Sloan, *A fast algorithm for generating constrained Delaunay triangulations*, Computers
  & Structures 47(3):441–450, 1993 — recovering the intersection segments in each triangle a
  Boolean splits, by flipping the edges that cross them (`emcraft-mesh::csg`).
- A. Van Oosterom, J. Strackee, *The solid angle of a plane triangle*, IEEE Trans. Biomed. Eng.
  30(2):125–126, 1983 — the winding number that classifies a Boolean's pieces inside or outside
  the other surface (`emcraft-mesh::csg`), and which pieces of a part's enclosed air are which
  (`emcraft-mesh::cavity`).
- H. Hoppe, T. DeRose, T. Duchamp, J. McDonald, W. Stuetzle, *Mesh optimization*, Proc. SIGGRAPH
  1993, pp. 19–26; T. K. Dey, H. Edelsbrunner, S. Guha, D. V. Nekhayev, *Topology preserving edge
  contraction*, Publ. Inst. Math. (Beograd) 66(80):23–45, 1999 — edge collapses under the link
  condition, which remove the points a Boolean leaves inside straight creases without changing
  the surface (`emcraft-mesh::crease`).
- Exact surfaces of imported faces (`emcraft-io::fit`): G. Lukács, R. Martin, D. Marshall,
  *Faithful least-squares fitting of spheres, cylinders, cones and tori for reliable
  segmentation*, ECCV 1998, LNCS 1406:671–686 — geometric distances refined by
  Levenberg–Marquardt from algebraic starts; D. Eberly, *Least Squares Fitting of Data by Linear
  or Quadratic Structures*, Geometric Tools, 2021 — the total least-squares plane and the
  cylinder through its axis; I. Kåsa, *A circle fitting procedure and its error analysis*, IEEE
  Trans. Instrum. Meas. 25(1):8–14, 1976, and V. Pratt, *Direct least-squares fitting of
  algebraic surfaces*, Computer Graphics 21(4):145–152, 1987 — the algebraic circle and sphere
  fits that start them. The axes of cylinders and cones come from the Gauss map (a cylinder's
  normals lie on a great circle, a cone's on a small one); a fit is kept only when every vertex
  lies on it to 1e-6 of the body's size and every triangle's centroid within 0.15 of its longest
  edge, the closest to the centroids winning, so the sphere through a cylinder's or a frustum's
  two rims is not taken for it. A torus' axis comes from the normal lines instead: H. Pottmann,
  T. Randrup, *Rotational and helical surface approximation for reverse engineering*, Computing
  60(4):307–322, 1998 — a surface of revolution's normals all meet its axis, a linear condition
  on the axis' Plücker coordinates solved as the least eigenvector of a Schur complement; a torus
  is tried only where no plane, cylinder, sphere or cone fits (each is a torus' limit).

## Special functions
- M. Abramowitz, I. A. Stegun, *Handbook of Mathematical Functions*, 1964, §9.12 and Tables 9.5,
  10.6 — Bessel functions by backward recurrence, their zeros (`emcraft-fem::bessel`, used by
  `emcraft-theory::bessel` and the circular ports).
- R. F. Harrington, *Time-Harmonic Electromagnetic Fields*, McGraw-Hill 1961, §6.10 — the
  resonances of a spherical cavity.

## Elements and discretisation
- H. Whitney, *Geometric Integration Theory*, Princeton University Press 1957 — the lowest-order
  edge functions λ_i∇λ_j − λ_j∇λ_i (`emcraft-fem::nedelec`, order 1).
- J.-C. Nédélec, *Mixed finite elements in R³*, Numer. Math. 35:315–341, 1980 — the
  H(curl)-conforming spaces of the first kind.
- P. Monk, *Finite Element Methods for Maxwell's Equations*, Oxford 2003, §3.9 — the covariant
  Piola transform N = DΦ^{−T} N̂, curl N = DΦ curl N̂ / det DΦ that carries edge elements to
  curved (isoparametric) tetrahedra (`emcraft-fem::nedelec`, `Element::curved`).
- J. P. Webb, *Hierarchal vector basis functions of arbitrary order for triangular and
  tetrahedral finite elements*, IEEE Trans. Antennas Propag. 47(8):1244–1253, 1999 — the order 2
  basis (edge gradients ∇(λ_iλ_j), face functions λ_i w_jk).
- Z. J. Cendes, *Vector finite elements for electromagnetic field computation*, IEEE Trans. Magn.
  27(5):3958–3966, 1991 — element matrices from barycentric integration.
- J.-M. Jin, *The Finite Element Method in Electromagnetics*, 3rd ed., Wiley 2014 — the
  curl–curl formulation, waveguide port conditions with modal expansions (§8.5, §9.2).
- H. W. Kuhn, *Some combinatorial lemmas in topology*, IBM J. Res. Dev. 4(5):518–524, 1960 — the
  six-tetrahedra split of a cube (`emcraft-mesh`).

## Waveguides and references
- D. M. Pozar, *Microwave Engineering*, 4th ed., Wiley 2012 — rectangular waveguide modes (§3.3),
  S-parameters and power waves (§4.3–4.4).
- R. E. Collin, *Foundations for Microwave Engineering*, 2nd ed., McGraw-Hill 1992 — evanescent
  modes and transmission-line equivalents (§3.12, §4.2).
- J.-F. Lee, D.-K. Sun, Z. J. Cendes, *Full-wave analysis of dielectric waveguides using
  tangential vector finite elements*, IEEE Trans. MTT 39(8):1262–1271, 1991; J.-M. Jin, *The
  Finite Element Method in Electromagnetics*, 3rd ed., Wiley 2014, §8.4 — the modes of a port of
  any cross-section: edge functions for the transverse field, nodal ones for the axial, the
  generalised eigenproblem in −β² (`emcraft-solver::portmode`, elements in `emcraft-fem::tri`).
- R. E. Collin, *Field Theory of Guided Waves*, 2nd ed., IEEE Press 1991, §6.1; R. F.
  Harrington, *Time-Harmonic Electromagnetic Fields*, McGraw-Hill 1961, §4-7 — the LSE and LSM
  modes of a slab-loaded rectangular guide, the reference for hybrid port modes
  (`emcraft-theory::slab`); the same equations, entire in β² and εr, solved with a complex εr
  by following each lossless root as the loss grows (Newton's method), the reference for hybrid
  lossy port modes (`slab_loaded_modes_lossy`; oracle `lossy-slab-guide`).
- Hybrid lossy port modes (`emcraft-solver::portmode::lossy`): Lee–Sun–Cendes with complex εr
  and μr per triangle, a complex symmetric pencil solved by complex shift-invert subspace
  iteration; modes of distinct γ orthogonal in the unconjugated form ∫ μr⁻¹ e_m · g_n dS (L. B.
  Felsen, N. Marcuvitz, *Radiation and Scattering of Waves*, Prentice-Hall 1973, §2.2; Marks &
  Williams 1992, §2), degenerate clusters made so by Gram–Schmidt in that form; the port terms
  with γ for jβ and the unconjugated D, S as Marks–Williams pseudo-waves
  (`emcraft-solver::driven::terms`; oracles `lossy-stripline`, `lossy-slab-guide`,
  `lossy-microstrip`).
- D. M. Pozar, *Microwave Engineering*, 4th ed., Wiley 2012, §3.8 — the dielectric attenuation
  of a quasi-TEM line, α_d = k₀ εr (ε_eff − 1) tan δ / (2 √ε_eff (εr − 1)), the benchmark for a
  microstrip on a lossy substrate with Hammerstad–Jensen's ε_eff (oracle `lossy-microstrip`).
- E. Hammerstad, Ø. Jensen, *Accurate models for microstrip computer-aided design*, IEEE MTT-S
  International Microwave Symposium Digest, 1980, pp. 407–409 — microstrip's quasi-static
  effective permittivity and impedance, the benchmark for strips on a substrate
  (`emcraft-theory::microstrip`).
- S. B. Cohn, *Characteristic impedance of the shielded-strip transmission line*, IRE Trans. MTT
  2(2):52–57, 1954 — the zero-thickness stripline's exact impedance (`emcraft-theory::stripline`);
  its wide-strip limit K(k′)/K(k) → W/b + (2/π) ln 2 gives each edge's excess width b ln 2/π,
  the open end's extension in the oracle cases `stripline-ends` and `stripline-stub`.
- S. B. Cohn, *Shielded coupled-strip transmission line*, IRE Trans. MTT 3(5):29–38, 1955 —
  the even and odd impedances of edge-coupled zero-thickness strips by conformal mapping,
  kₑ = tanh(πW/2b) tanh(π(W + S)/2b), kₒ = tanh(πW/2b) coth(π(W + S)/2b), and its inversion
  from (Z₀ₑ, Z₀ₒ) to (W, S) (`emcraft-theory::coupled`; oracle `coupled-stripline-impedance`).
- C. R. Paul, *Analysis of Multiconductor Transmission Lines*, 2nd ed., Wiley 2008, §§5.2, 7.2 —
  a homogeneous line's L and C share their eigenvectors: the degenerate TEM modes of several
  strips are split along them, power-orthogonal with orthogonal conductor currents (the even and
  odd modes), and each mode's impedance per conductor 2P/Σ|I|² is the impedance matrix's
  eigenvalue (`emcraft-solver::portmode::coupled`).
- E. M. T. Jones, J. T. Bolljahn, *Coupled-strip-transmission-line filters and directional
  couplers*, IRE Trans. MTT 4(2):75–81, 1956; D. M. Pozar, *Microwave Engineering*, 4th ed.,
  Wiley 2012, §8.7 — the open-circuited coupled section's impedance matrix and chain matrix,
  the quarter-wave section an inverter of (Z₀ₑ − Z₀ₒ)/2 (oracle `coupled-line-coupler`).
- S. B. Cohn, *Parallel-coupled transmission-line-resonator filters*, IRE Trans. MTT
  6(2):223–231, 1958; G. L. Matthaei, L. Young, E. M. T. Jones, *Microwave Filters,
  Impedance-Matching Networks, and Coupling Structures*, McGraw-Hill 1964, §§4.05, 10.02 — the
  Chebyshev prototype's g values, the admittance inverters J of a parallel-coupled band-pass
  filter and their even and odd impedances Z₀[1 ± JZ₀ + (JZ₀)²]; the ideal TEM cascade the
  `coupled-filter` sample and oracle `coupled-line-bandpass` are checked against.
- A. G. Altschuler, A. A. Oliner, *Discontinuities in the center conductor of symmetric strip
  transmission line*, IRE Trans. MTT 8(3):328–339, 1960 — the open end of a stripline, its
  quasi-static extension that of a side edge; A. A. Oliner, *Equivalent circuits for
  discontinuities in balanced strip transmission line*, IRE Trans. MTT 3(2):134–143, 1955 — the
  T-junction's equivalent circuit, what the stubs' line model leaves out (`stripline-stub`).
- K. Kurokawa, *Power waves and the scattering matrix*, IEEE Trans. MTT 13(2):194–202, 1965 — S
  referred to reference impedances from the modes' power waves (`emcraft-solver::driven`).
- D. F. Williams, L. A. Hayden, R. B. Marks, *A complete multimode equivalent-circuit theory for
  electrical design*, J. Res. NIST 102(4):405–423, 1997 — modal voltages and currents of a
  multiconductor port, the power-current definition V = 2 (T_Iᴴ)⁻¹ that makes the conductors'
  power the modes'; G. Faché, F. Olyslager, D. De Zutter, *Electromagnetic and Circuit Modelling
  of Multiconductor Transmission Lines*, Oxford 1993, ch. 4 — the modal transformation of a
  multiconductor line to its conductors: terminal ports, one per strip, their power waves by
  Kurokawa's definition (`emcraft-solver::driven::terminals`).
- D. M. Pozar, *Microwave Engineering*, 4th ed., Wiley 2012, §7.6 — the coupled-line
  directional coupler by its even and odd modes, matched and isolated between ports of
  √(Z₀ₑZ₀ₒ), coupled and through waves 90° apart (`emcraft-theory::coupled::coupler`; oracle
  `coupled-line-coupler-4port`, the `coupler` sample).
- J. Meixner, *The behavior of electromagnetic fields at edges*, IEEE Trans. AP 20(4):442–446,
  1972 — E ∝ r^{−1/2} at a strip's edge, why the structured mesh closes in harder there than at
  an iris' (`emcraft-mesh`, `strip_edge`).

## Discontinuities
- A. Wexler, *Solution of waveguide discontinuities by modal analysis*, IEEE Trans. MTT
  15(9):508–517, 1967; T. Itoh (ed.), *Numerical Techniques for Microwave and Millimeter-Wave
  Passive Structures*, Wiley 1989, ch. 9; J. Uher, J. Bornemann, U. Rosenberg, *Waveguide
  Components for Antenna Feed Systems: Theory and CAD*, Artech House 1993 — mode matching and
  generalised scattering matrices (`emcraft-theory::modematch`).
- R. Mittra, T. Itoh, T.-S. Li, *Analytical and numerical studies of the relative convergence
  phenomenon arising in the solution of an integral equation by the moment method*, IEEE Trans.
  MTT 20(2):96–104, 1972 — mode counts in proportion to the apertures.
- J. Meixner, *The behavior of electromagnetic fields at edges*, IEEE Trans. Antennas Propag.
  20(4):442–446, 1972 — the r^{-1/3} field at a thin edge; I. Babuška, R. B. Kellogg, J.
  Pitkäranta, *Direct and inverse error estimates for finite elements with mesh refinements*,
  Numer. Math. 33:447–471, 1979 — geometric grading towards singularities (`emcraft-mesh`).
- N. Marcuvitz, *Waveguide Handbook*, McGraw-Hill 1951, §5.11; L. Lewin, *Theory of Waveguides*,
  Butterworth 1975, ch. 6; R. E. Collin, *Field Theory of Guided Waves*, 2nd ed., IEEE Press
  1991, ch. 5 — the full-height post as a two-dimensional problem, the parallel-plate Green's
  function in modes with its static part summed in closed form (Kummer); R. Kress, *Linear
  Integral Equations*, 3rd ed., Springer 2014, ch. 12–13, and D. Colton, R. Kress, *Inverse
  Acoustic and Electromagnetic Scattering Theory*, 3rd ed., Springer 2013, §3.6 — the Nyström
  method with logarithmic-split weights, exponentially convergent on a smooth boundary
  (`emcraft-theory::post`, the reference of the oracle's inductive post).
- A. Sieverding, F. Arndt, *Field theoretic CAD of open or aperture matched T-junction coupled
  rectangular waveguide structures*, IEEE Trans. MTT 40(2):353–362, 1992; J. Uher, J.
  Bornemann, U. Rosenberg, *Waveguide Components for Antenna Feed Systems: Theory and CAD*,
  Artech House 1993 — field superposition in a junction's region: its field the sum of
  expansions each closed by one of its walls, matched to the guides' modes on its open sides
  (`emcraft-theory::bend`, the square-cornered H-plane and E-plane bends, references of the
  oracle's `hplane-corner` and `eplane-corner`). The E-plane corner as a 2D Neumann problem of
  the LSE modes: R. E. Collin, *Field Theory of Guided Waves*, 2nd ed., IEEE Press 1991; N.
  Marcuvitz, *Waveguide Handbook*, McGraw-Hill 1951, ch. 5 — right-angle corners.
- D. M. Pozar, *Microwave Engineering*, 4th ed., Wiley 2012, §5.8–5.9; R. E. Collin,
  *Foundations for Microwave Engineering*, 2nd ed., McGraw-Hill 1992, §5.10 — the theory of
  small reflections, Γ = ½ ∫ (d ln Z/dz) e^{−2jθ} dz, here for a linear taper between
  rectangular guides with the TE10 line's Z ∝ b/β and θ in closed form (`emcraft-theory::taper`,
  the reference of the oracle's `design-taper`).

## Ports on any axis
- A port's modes and cross-section live in its own axes (u, v, w): the face's two other axes in
  the order x, y, z and w = u × v, a proper rotation of a z port, so the closed forms and the
  cross-section solver (J.-F. Lee, D.-K. Sun, Z. J. Cendes, IEEE Trans. MTT 39(8), 1991) apply
  unchanged; the modal port condition is the same on any face (Jin 2014, §8.5)
  (`emcraft-model::port::Frame`, `emcraft-solver::driven::ports`).
- G. H. Meisters, *Polygons have ears*, Amer. Math. Monthly 82(6):648–651, 1975; D. Eberly,
  *Triangulation by Ear Clipping*, Geometric Tools, 2002 — a prism's caps cut into triangles
  (`emcraft-model::prism`, the outlines of bends, mitred corners and tees).

## Symmetry planes
- J.-M. Jin, *The Finite Element Method in Electromagnetics*, 3rd ed., Wiley 2014, ch. 1 and
  ch. 8 — the electric wall (tangential E = 0) as the essential condition of edge elements, its
  tangential unknowns eliminated, and the magnetic wall (tangential H = 0) as the natural one,
  its unknowns kept with no surface term (`emcraft-model::Symmetry`, `emcraft-solver::driven`).
- R. F. Harrington, *Time-Harmonic Electromagnetic Fields*, McGraw-Hill 1961, ch. 3 — duality
  and images: a field even or odd about a plane of a symmetric model is the half model's closed
  there by a magnetic or an electric wall, and beyond the plane the image of the half's (the
  ports' modes of that parity; fields shown beyond the plane).

## Losses
- T. B. A. Senior, J. L. Volakis, *Approximate Boundary Conditions in Electromagnetics*, IEE 1995,
  §2.2 — the Leontovich surface-impedance condition for walls of finite conductivity.
- D. M. Pozar, *Microwave Engineering*, 4th ed., Wiley 2012, eq. 3.96 — TE10 conductor
  attenuation (the reference); §1.3 — the loss tangent, dielectric ε = ε′ − jε″ and magnetic
  μ = μ′ − jμ″ alike (`emcraft-model::Material`, `magnetic_loss_tangent`); §3.3 — the modes of a
  guide filled with a lossy medium, γ = √(k_c² − k²) with complex k² (the references of the
  oracle cases `lossy-line`, `magnetic-loss-line`, `lossy-step`).
- J.-M. Jin, *The Finite Element Method in Electromagnetics*, 3rd ed., Wiley 2014, §8.5 — the
  modal port condition n̂ × ∇ × E_t = κ E_t, which holds with the complex γ and κ of a port's
  lossy filling; magnetic loss as the complex μr⁻¹ of the curl–curl term
  (`emcraft-solver::driven::ports`, `terms`).
- R. B. Marks, D. F. Williams, *A general waveguide circuit theory*, J. Res. NIST 97(5):533–562,
  1992 — pseudo-waves for lossy lines, whose characteristic impedance is complex: power waves
  of a lossy line are not reciprocal, waves referred to the line's own characteristic
  impedance are. EMCraft reads S at lossy ports as such waves, normalised by the modes'
  unconjugated power ∫ e × h · ŝ dS (√(P_p/P_q) on the principal branch), so S stays
  symmetric for a reciprocal structure and reduces to the power waves for lossless ports
  (`emcraft-solver::driven::sparams`).
- A. Taflove, S. C. Hagness, *Computational Electrodynamics: The Finite-Difference Time-Domain
  Method*, 3rd ed., Artech House 2005, ch. 9 (§9.2 Debye, §9.4 Lorentz and Drude media) — the
  dispersive permittivities ε(ω) = ε∞ + Σ χ_k(ω), written for e^{jωt}: Debye
  Δε/(1 + jωτ), Lorentz Δε ω₀²/(ω₀² − ω² + jωγ), Drude −ω_p²/(ω² − jωγ)
  (`emcraft-model::Pole`). The frequency-domain solver evaluates them at each frequency, each
  dispersive material with its own mass matrix (`emcraft-solver::driven`, and term by term in
  the fast sweep).

## Sparse direct solution
- A. George, *Nested dissection of a regular finite element mesh*, SIAM J. Numer. Anal.
  10(2):345–363, 1973; H. D. Simon, *Partitioning of unstructured problems for parallel
  processing*, Computing Systems in Engineering 2:135–148, 1991 — the ordering.
- J. W. H. Liu, *The role of elimination trees in sparse factorization*, SIAM J. Matrix Anal.
  Appl. 11(1):134–172, 1990 — elimination trees and postorders.
- I. S. Duff, J. K. Reid, *The multifrontal solution of indefinite sparse symmetric linear
  equations*, ACM TOMS 9(3):302–325, 1983; J. W. H. Liu, *The multifrontal method for sparse
  matrix solution: theory and practice*, SIAM Review 34(1):82–109, 1992 — the multifrontal
  method.
- C. Ashcraft, R. Grimes, *The influence of relaxed supernode partitions on the multifrontal
  method*, ACM TOMS 15(4):291–309, 1989; Y. Chen, T. A. Davis, W. W. Hager, S. Rajamanickam,
  *Algorithm 887: CHOLMOD*, ACM TOMS 35(3):22, 2008 — relaxed amalgamation and its thresholds.
- J. R. Bunch, L. Kaufman, *Some stable methods for calculating inertia and solving symmetric
  linear systems*, Math. Comp. 31(137):163–179, 1977 — 1 × 1 and 2 × 2 pivots.
- O. Schenk, K. Gärtner, *Solving unsymmetric sparse systems of linear equations with PARDISO*,
  FGCS 20(3):475–487, 2004 — static pivoting with iterative refinement.
- A. van der Sluis, *Condition numbers and equilibration of matrices*, Numer. Math. 14:14–23,
  1969 — the symmetric scaling S A S, each row's largest entry one, before the factorization:
  the static-pivot threshold then means the same for every unknown of a graded mesh.

## Time domain
- K. S. Yee, *Numerical solution of initial boundary value problems involving Maxwell's
  equations in isotropic media*, IEEE Trans. Antennas Propag. 14(3):302–307, 1966; T. Weiland,
  *A discretization method for the solution of Maxwell's equations for six-component fields*,
  AEÜ 31:116–120, 1977 — the scheme and its integral (FIT) form on non-uniform grids.
- J. A. Roden, S. D. Gedney, *Convolution PML (CPML): an efficient FDTD implementation of the
  CFS–PML for arbitrary media*, Microw. Opt. Technol. Lett. 27(5):334–339, 2000; A. Taflove, S.
  C. Hagness, *Computational Electrodynamics: The FDTD Method*, 3rd ed., Artech House 2005,
  ch. 7 — the absorbing layers and their grading.
- A. Taflove, S. C. Hagness, *Computational Electrodynamics*, 3rd ed., Artech House 2005, §9.4 —
  the auxiliary differential equation (ADE) method for dispersive media: each pole's
  polarization advanced with E on the edges it touches, D = ε₀ε∞E + ΣP; Debye poles centred at
  n + ½ (trapezoidal, solved per edge with E), Lorentz and Drude poles centred at n (explicit)
  (`emcraft-fdtd`).
- A. Taflove, S. C. Hagness, *Computational Electrodynamics*, 3rd ed., Artech House 2005,
  §3.12 — lossy media: the conduction current averaged over the step (semi-implicit) and the
  exponential update; EMCraft solves the first with ε scaled by (r/2) coth(r/2), r = σΔt/ε,
  which is the second, for electric and magnetic conductivities. A loss tangent becomes the
  conductivity matched at the band's centre, so the material solved is εr − jσ/ω
  (`emcraft-fdtd`, `Losses`).
- J. G. Maloney, G. S. Smith, *The use of surface impedance concepts in the finite-difference
  time-domain method*, IEEE Trans. Antennas Propag. 40(1):38–48, 1992; K. S. Oh, J. E.
  Schutt-Ainé, *An efficient implementation of surface impedance boundary conditions for the
  finite-difference time-domain method*, IEEE Trans. Antennas Propag. 43(7):660–666, 1995 —
  surface-impedance walls in the time domain by recursive convolution with sums of
  exponentials. EMCraft writes the Leontovich admittance √(σ/μ₀) s^{−1/2} by its Stieltjes
  integral (1/π)∫ x^{−1/2}/(s + x) dx, discretised by the trapezoidal rule in ln x (L. N.
  Trefethen, J. A. C. Weideman, *The exponentially convergent trapezoidal rule*, SIAM Review
  56(3):385–458, 2014) with Euler–Maclaurin ends and moment-matched tails: some 25 passive
  first-order branches of surface current per wall edge, each advanced by the trapezoidal rule,
  the edge updated by Ampère's law over the half of its dual face inside (`emcraft-fdtd`,
  `surface`).
- R. B. Marks, D. F. Williams, *A general waveguide circuit theory*, J. Res. NIST
  97(5):533–562, 1992 — the time domain's ports in lossy and dispersive media: S₂₁ normalised by
  the modes' unconjugated powers ∝ β/μ with the complex TE10 β, as the frequency-domain
  solver's pseudo-waves.
- C. A. Balanis, *Advanced Engineering Electromagnetics*, 2nd ed., Wiley 2012, §6.8 — the
  Hertzian dipole's field, r H_φ ∝ k ζ(kr): the reference for a dipole at the centre of a
  lossy, magnetic or dispersive sphere in the time domain's open problems (the TM₀₁ spherical
  wave matched at the sphere's surface).

## Adaptive refinement
- P. Monk, *A posteriori error indicators for Maxwell's equations*, J. Comput. Appl. Math.
  100(2):173–190, 1998; P. Monk, *Finite Element Methods for Maxwell's Equations*, Oxford 2003,
  ch. 12; R. Beck, R. Hiptmair, R. H. W. Hoppe, B. Wohlmuth, *Residual based a posteriori error
  estimators for eddy current computation*, M2AN 34(1):159–182, 2000 — the residual indicator
  for the curl–curl equation on Nédélec elements: the element residual and the face jumps of
  tangential μ⁻¹∇×E and of normal εE, weighted by the element and face sizes
  (`emcraft-solver::adapt`; the element divergence term left out, zero for Whitney elements).
- W. Dörfler, *A convergent adaptive algorithm for Poisson's equation*, SIAM J. Numer. Anal.
  33(3):1106–1124, 1996; P. Morin, R. H. Nochetto, K. G. Siebert, *Data oscillation and
  convergence of adaptive FEM*, SIAM J. Numer. Anal. 38(2):466–488, 2000 — bulk (Dörfler)
  marking of the fewest elements holding a fraction θ of the estimate (`emcraft-mesh::adapt`).
  The marked tetrahedra are refined by remeshing with local sizes graded by the mesher's
  background grid (`emcraft-tetmesh::tet::geometry::GradedSize`), not by splitting.
- Z. Chen, X. Liu, *An adaptive perfectly matched layer technique for time-harmonic
  scattering problems*, SIAM J. Numer. Anal. 43(2):645–671, 2005 — adaptivity for open
  problems estimates the error in the physical domain and leaves the layer's own to its design;
  `mesh.adapt` on an open problem leaves the layer's tetrahedra (and every face they touch) out
  of the indicator, so the layer is never marked (`emcraft-solver::adapt::open_indicator`).
- M. Ainsworth, J. T. Oden, *A Posteriori Error Estimation in Finite Element Analysis*, Wiley
  2000, ch. 2 — residual estimators with their data: a plane wave's scattered field takes
  k₀²(εr − 1)E_inc as the element residual's source and the incident flux jump on interfaces,
  its conductors' lifted trace −n × E_inc as boundary data; a current element's or line gap's
  singular field (not square-integrable) is left out with the tetrahedra holding it.

## Fast frequency sweeps
- P. Benner, M. Hinze, E. J. W. ter Maten (eds.), *Model Reduction for Circuit Simulation*,
  Springer 2011, ch. 1; A. T. Patera, G. Rozza, *Reduced Basis Approximation and A Posteriori
  Error Estimation for Parametrized Partial Differential Equations*, MIT 2007 — the greedy
  reduced basis: solved fields span the basis, the residual chooses the next frequency to solve
  (`emcraft-solver::sweep`).

## Eigenvalues
- K.-J. Bathe, E. L. Wilson, *Large eigenvalue problems in dynamic analysis*, J. Eng. Mech. Div.
  ASCE 98(6):1471–1485, 1972; K.-J. Bathe, *Finite Element Procedures*, 2nd ed. 2014, §11.6 —
  subspace iteration with Rayleigh–Ritz.
- G. H. Golub, C. F. Van Loan, *Matrix Computations*, 4th ed., Johns Hopkins 2013, §8.5 — the
  cyclic Jacobi method for the reduced problem; §7.4–7.6 — Householder reduction to Hessenberg
  form, the Francis double-shift QR algorithm and inverse iteration, for the nonsymmetric
  projected problems of the port modes (`emcraft-solver::dense`); in complex arithmetic the
  single-shift QR with the Wilkinson shift, a trailing 2 × 2 block that will not deflate (a
  degenerate pair coupled by rounding) solved directly, for lossy cavities and hybrid lossy port
  modes (`emcraft-solver::zdense`).
- D. M. Pozar, *Microwave Engineering*, 4th ed., Wiley 2012, §6.3 and §6.7 — cavity resonances,
  Q by perturbation, the TE101 Q (eq. 6.46).

## Open problems
- Z. S. Sacks, D. M. Kingsland, R. Lee, J.-F. Lee, *A perfectly matched anisotropic absorber for
  use as an absorbing boundary condition*, IEEE Trans. Antennas Propag. 43(12):1460–1463, 1995;
  J.-M. Jin, *The Finite Element Method in Electromagnetics*, 3rd ed., §9.6 — the layer as the
  medium εΛ, μΛ of stretched coordinates (`emcraft-solver::driven`, `add_pml`).
- W. C. Chew, W. H. Weedon, *A 3D perfectly matched medium from modified Maxwell's equations
  with stretched coordinates*, Microw. Opt. Technol. Lett. 7(13):599–604, 1994 — complex
  coordinate stretching; F. L. Teixeira, W. C. Chew, *Systematic derivation of anisotropic PML
  absorbing media in cylindrical and spherical coordinates*, IEEE Microw. Guided Wave Lett.
  7(11):371–373, 1997 — the spherical layer: the radius alone stretched, r̃ = r + (1/jk₀)∫σ dr,
  is the Cartesian medium εΛ, μΛ with Λ = (r̃/r)² s_r⁻¹ r̂r̂ + s_r (θ̂θ̂ + φ̂φ̂), its axes turning
  with r̂ (evaluated at each quadrature point: `emcraft-solver::driven`, `add_spherical_pml`).
- C. A. Balanis, *Advanced Engineering Electromagnetics*, 2nd ed., Wiley 2012, §6.8 — the
  near-to-far transform of equivalent currents; *Antenna Theory*, 4th ed., §4.2 — the Hertzian
  dipole (`emcraft-solver::radiation`, `emcraft-theory::radiation`).
- J.-M. Jin, *The Finite Element Method in Electromagnetics*, 3rd ed., §9.4 — lumped ports as
  currents across a gap behind an impedance; C. A. Balanis, *Antenna Theory*, §8.5 — the
  induced-EMF impedance of a dipole and the sine and cosine integrals (Abramowitz & Stegun
  §5.2) (`emcraft-solver::driven`, `emcraft-theory::radiation`).
- M. Piket-May, A. Taflove, J. Baron, *FD-TD modeling of digital signal propagation in 3-D
  circuits with passive and active loads*, IEEE Trans. MTT 42(8):1514–1523, 1994 — resistive
  voltage sources (lumped ports) on Yee edges; K. R. Umashankar, A. Taflove, B. Beker, IEEE
  Trans. AP 35(11), 1987 — thin wires as lines of edges (`emcraft-fdtd::open`).
- C. F. Bohren, D. R. Huffman, *Absorption and Scattering of Light by Small Particles*, Wiley
  1983, §4.4 and Appendix A — the Mie series and its BHMIE test values
  (`emcraft-theory::radiation`).
- C. A. Balanis, *Antenna Theory*, 3rd ed., Wiley 2005, §13.2–13.4 — the sectoral and
  pyramidal horns' directivity from the aperture's quadratic phase, D_p = (πλ²/32ab) D_E D_H,
  through the Fresnel integrals (M. Abramowitz, I. A. Stegun, *Handbook of Mathematical
  Functions*, 1964, §7.3, table 7.7 for their test values) (`emcraft-theory::horn`, the
  reference of the oracle's `design-horn`).

## Parameter studies
- J. A. Nelder, R. Mead, *A simplex method for function minimization*, Comput. J. 7(4):308–313,
  1965 — the downhill simplex `study.optimize` runs on the variables scaled to [0, 1], trial
  points clamped into the box; J. C. Lagarias, J. A. Reeds, M. H. Wright, P. E. Wright,
  *Convergence properties of the Nelder–Mead simplex method in low dimensions*, SIAM J. Optim.
  9(1):112–147, 1998 — the coefficients (1, 2, ½, ½), the order of the tests and the tie rules
  (`emcraft-engine::study`).
- D. M. Pozar, *Microwave Engineering*, 4th ed., Wiley 2012, §2.5 and §5.4 — the quarter-wave
  transformer the optimizer's oracle case is checked against.
## Periodic structures
- G. Floquet, *Sur les équations différentielles linéaires à coefficients périodiques*, Ann.
  Sci. ÉNS 12:47–88, 1883 — fields of a periodic structure as E(r + L) = e^{−jψ}E(r).
- J.-M. Jin, *The Finite Element Method in Electromagnetics*, 3rd ed., Wiley 2014, the chapter on
  periodic structures — the unit cell with phase-shifted boundary conditions, Floquet ports and
  the phase-shift eigenproblem (`emcraft-solver::driven::periodic`, `driven::floquet`, `bloch`).
- D. T. McGrath, V. P. Pyati, *Phased array antenna analysis with the hybrid finite element
  method*, IEEE Trans. Antennas Propag. 42(12):1625–1630, 1994 — master/slave elimination of the
  periodic faces' unknowns. Ours is applied to the assembled matrix, x = T x_r, with the test
  functions of conjugate phase (Tᴴ A T): Hermitian for a lossless cell, not complex symmetric
  unless the phase is real, so the reduced system is factored by LU.
- A. K. Bhattacharyya, *Phased Array Antennas: Floquet Analysis, Synthesis, BFNs, and Active
  Array Systems*, Wiley 2006 — Floquet harmonics, their TE/TM polarizations and orthogonality
  under the conjugate product (the Floquet ports).
- I. S. Duff, J. K. Reid, *The multifrontal solution of unsymmetric sets of linear equations*,
  SIAM J. Sci. Stat. Comput. 5(3):633–641, 1984 — multifrontal LU on a symmetric pattern, pivots
  within each front's fully summed block (`emcraft-solver::lu`).
- X. S. Li, J. W. Demmel, *Making sparse Gaussian elimination scalable by static pivoting*,
  Proc. SC98, 1998 — tiny pivots replaced, accuracy recovered by iterative refinement.
- J. D. Joannopoulos, S. G. Johnson, J. N. Winn, R. D. Meade, *Photonic Crystals: Molding the
  Flow of Light*, 2nd ed., Princeton 2008 — band structures by the eigenproblem at each Bloch
  wavevector (`solve.dispersion`).
- References: S. J. Orfanidis, *Electromagnetic Waves and Antennas*, Rutgers 2016, ch. 7–8
  (plane waves through layers at oblique incidence, transverse impedances); D. M. Pozar,
  *Microwave Engineering*, 4th ed., §4.4 and §8.1, and R. E. Collin, *Foundations for Microwave
  Engineering*, 2nd ed., periodic structures (Bloch waves of a periodically loaded line,
  cos ψ = (A + D)/2) — `emcraft-theory::floquet`.

## Quadrature
- A. H. Stroud, *Approximate Calculation of Multiple Integrals*, Prentice-Hall 1971; M. G. Duffy,
  *Quadrature over a pyramid or cube of integrands with a singularity at a vertex*, SIAM J.
  Numer. Anal. 19(6):1260–1262, 1982 — collapsed (conical product) rules on simplices.

## Time-domain reflectometry

- J. Dunsmore, *Handbook of Microwave Component Measurements*, 2nd ed., Wiley 2020, ch. 4: the
  time-domain transforms of a network analyser — low pass on a harmonic grid with DC
  extrapolated (step response), band pass (impulse envelope), windows. `crates/engine/src/tdr.rs`.

## 3D view

- J. D. Foley, A. van Dam, S. K. Feiner, J. F. Hughes, *Computer Graphics: Principles and
  Practice*, 2nd ed., ch. 3 and 15, and J. Pineda, *A parallel algorithm for polygon
  rasterization*, SIGGRAPH 1988 — the depth-buffered scan conversion; T. Möller, B. Trumbore,
  *Fast, minimum storage ray–triangle intersection*, J. Graphics Tools 2(1), 1997 — picking.
  `crates/render/src/view3d.rs`.
- Section views: I. E. Sutherland, G. W. Hodgman, *Reentrant polygon clipping*, Comm. ACM
  17(1), 1974 — clipping against the cut plane; D. Dobkin, R. J. Lipton, *Multidimensional
  searching problems*, SIAM J. Comput. 5(2), 1976, and F. P. Preparata, M. I. Shamos,
  *Computational Geometry: An Introduction*, Springer 1985, §2.2.2.1 — the slab method that
  fills the caps; E. Haines, *Point in polygon strategies*, Graphics Gems IV, 1994 — the
  even–odd rule; J. F. Hughes, T. Möller, *Building an orthonormal basis from a unit vector*,
  J. Graphics Tools 4(4), 1999 — the plane's coordinates. `crates/render/src/view3d/section.rs`.
