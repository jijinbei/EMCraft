# Electrons through a pillbox

Electrons tracked (`results.track`, docs/adr/0012-particles.md) through the TM010 mode of the
copper pillbox of `pillbox-accelerating` (10 mm in radius, 13.06 mm long: half a period for a
particle at the speed of light), at an accelerating voltage V₀ = |∫E_z dz| of 1 MV, entering
on the axis at twelve phases. A particle crossing at constant speed gains qV₀T cos φ
(Wangler, *RF Linear Accelerators*, §2.2–2.3), T = sin x / x with x = ωL/(2βc); at 100 MeV
(β = 0.99999) the gain, 0.64 MeV, hardly changes the speed, so the closed form holds to the
tolerance. A second run sends 1 MeV electrons (β = 0.941) through at 1 kV: T at their speed,
0.596 against 0.637.

The mode's phase is set by `results.track` so that φ = 0 gains the most at the entry speed;
the push samples the field at its own steps, which leaves the zero of the gain about 4e-4 of
V₀T off φ = 90°.
