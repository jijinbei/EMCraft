//! A plan carries everything a device needs: a plain serial stepper written from the plan's
//! documentation alone gives the CPU's S-parameters.

use std::ops::ControlFlow;

use emcraft_fdtd::{Cpml, Options, Plan, Stepper, s_parameters, s_parameters_with};
use emcraft_model::{End, Material, Model, Port, Section, Waveguide};

/// A length (m), checked.
fn metres(v: f64) -> emcraft_model::Metres {
    emcraft_model::Metres::new(v).unwrap()
}

/// The leapfrog of `Plan`'s documentation, one loop per component.
struct Serial;

fn at(dims: [usize; 3], i: usize, j: usize, k: usize) -> usize {
    i + dims[0] * (j + dims[1] * k)
}

impl Stepper for Serial {
    fn leapfrog(&self, p: &Plan, max_steps: usize, observe: &mut dyn FnMut(usize, [f64; 2]) -> ControlFlow<()>) -> emcraft_fdtd::Result<usize> {
        let [nx, ny, nz] = p.n;
        let de = [[nx, ny + 1, nz + 1], [nx + 1, ny, nz + 1], [nx + 1, ny + 1, nz]];
        let dh = [[nx + 1, ny, nz], [nx, ny + 1, nz], [nx, ny, nz + 1]];
        let mut e = de.map(|d| vec![0.0; d.iter().product()]);
        let mut h = dh.map(|d| vec![0.0; d.iter().product()]);
        let (mut psi_ex, mut psi_ey) = (vec![0.0; e[0].len()], vec![0.0; e[1].len()]);
        let (mut psi_hx, mut psi_hy) = (vec![0.0; h[0].len()], vec![0.0; h[1].len()]);
        let ([dx, dy, dz], [dxd, dyd, dzd]) = (&p.primal, &p.dual);
        for n in 0..max_steps {
            for k in 0..=nz {
                for j in 0..=ny {
                    for i in 0..=nx {
                        if j < ny && k < nz {
                            let id = at(dh[0], i, j, k);
                            if p.h[0][id] != 0.0 {
                                let dez_dy = (e[2][at(de[2], i, j + 1, k)] - e[2][at(de[2], i, j, k)]) / dy[j];
                                let dey_dz = (e[1][at(de[1], i, j, k + 1)] - e[1][at(de[1], i, j, k)]) / dz[k];
                                let Cpml { b, a } = p.cpml_h[k];
                                psi_hx[id] = b * psi_hx[id] + a * dey_dz;
                                h[0][id] -= p.h[0][id] * (dez_dy - dey_dz - psi_hx[id]);
                            }
                        }
                        if i < nx && k < nz {
                            let id = at(dh[1], i, j, k);
                            if p.h[1][id] != 0.0 {
                                let dex_dz = (e[0][at(de[0], i, j, k + 1)] - e[0][at(de[0], i, j, k)]) / dz[k];
                                let dez_dx = (e[2][at(de[2], i + 1, j, k)] - e[2][at(de[2], i, j, k)]) / dx[i];
                                let Cpml { b, a } = p.cpml_h[k];
                                psi_hy[id] = b * psi_hy[id] + a * dex_dz;
                                h[1][id] -= p.h[1][id] * (dex_dz + psi_hy[id] - dez_dx);
                            }
                        }
                        if i < nx && j < ny {
                            let id = at(dh[2], i, j, k);
                            if p.h[2][id] != 0.0 {
                                let dey_dx = (e[1][at(de[1], i + 1, j, k)] - e[1][at(de[1], i, j, k)]) / dx[i];
                                let dex_dy = (e[0][at(de[0], i, j + 1, k)] - e[0][at(de[0], i, j, k)]) / dy[j];
                                h[2][id] -= p.h[2][id] * (dey_dx - dex_dy);
                            }
                        }
                    }
                }
            }
            for k in 0..=nz {
                for j in 0..=ny {
                    for i in 0..=nx {
                        if i < nx && j >= 1 && j < ny && k >= 1 && k < nz {
                            let id = at(de[0], i, j, k);
                            if p.e[0][id] != 0.0 {
                                let dhz_dy = (h[2][at(dh[2], i, j, k)] - h[2][at(dh[2], i, j - 1, k)]) / dyd[j];
                                let dhy_dz = (h[1][at(dh[1], i, j, k)] - h[1][at(dh[1], i, j, k - 1)]) / dzd[k];
                                let Cpml { b, a } = p.cpml_e[k];
                                psi_ex[id] = b * psi_ex[id] + a * dhy_dz;
                                e[0][id] += p.e[0][id] * (dhz_dy - dhy_dz - psi_ex[id]);
                            }
                        }
                        if i >= 1 && i < nx && j < ny && k >= 1 && k < nz {
                            let id = at(de[1], i, j, k);
                            if p.e[1][id] != 0.0 {
                                let dhx_dz = (h[0][at(dh[0], i, j, k)] - h[0][at(dh[0], i, j, k - 1)]) / dzd[k];
                                let dhz_dx = (h[2][at(dh[2], i, j, k)] - h[2][at(dh[2], i - 1, j, k)]) / dxd[i];
                                let Cpml { b, a } = p.cpml_e[k];
                                psi_ey[id] = b * psi_ey[id] + a * dhx_dz;
                                e[1][id] += p.e[1][id] * (dhx_dz + psi_ey[id] - dhz_dx);
                            }
                        }
                        if i >= 1 && i < nx && j >= 1 && j < ny && k < nz {
                            let id = at(de[2], i, j, k);
                            if p.e[2][id] != 0.0 {
                                let dhy_dx = (h[1][at(dh[1], i, j, k)] - h[1][at(dh[1], i - 1, j, k)]) / dxd[i];
                                let dhx_dy = (h[0][at(dh[0], i, j, k)] - h[0][at(dh[0], i, j - 1, k)]) / dyd[j];
                                e[2][id] += p.e[2][id] * (dhy_dx - dhx_dy);
                            }
                        }
                    }
                }
            }
            let s = p.source_at(n);
            let plane = (nx + 1) * ny;
            for m in 0..plane {
                let id = m + plane * p.source_plane;
                if p.e[1][id] != 0.0 {
                    e[1][id] += s * p.source[m];
                }
            }
            let project = |k: usize, w: &[f64]| (0..plane).fold(0.0, |s, m| s + e[1][m + plane * k] * w[m]);
            let v = [project(p.planes[0], &p.weights[0]), project(p.planes[1], &p.weights[1])];
            if observe(n, v).is_break() {
                return Ok(n + 1);
            }
        }
        Ok(max_steps)
    }
}

#[test]
fn a_plan_holds_the_whole_run() {
    let mut m = Model::new();
    m.add_material(Material::new("ptfe", 2.0, 1.0).unwrap()).unwrap();
    let sections = vec![Section::new(metres(0.015), "ptfe"), Section::new(metres(0.02), "vacuum"), Section::new(metres(0.015), "ptfe")];
    m.set_waveguide(Waveguide {
        width: metres(0.02286),
        height: metres(0.01016),
        sections,
        layers: Vec::new(),
        strips: Vec::new(),
        vias: Vec::new(),
    })
    .unwrap();
    m.add_port(Port::new("1", End::Start).with_modes(1)).unwrap();
    m.add_port(Port::new("2", End::End).with_modes(1)).unwrap();
    let (o, freqs) = (Options::new(0.003), [6.5e9, 8.5e9]);
    let cpu = s_parameters(&m, &o, &freqs).unwrap();
    let serial = s_parameters_with(&m, &o, &freqs, Some(&Serial)).unwrap();
    assert_eq!((serial.steps, serial.reference_steps), (cpu.steps, cpu.reference_steps));
    for i in 0..freqs.len() {
        // The same arithmetic but the projection's weights multiplied first: rounding apart.
        assert!((serial.s11[i] - cpu.s11[i]).norm() < 1e-10 && (serial.s21[i] - cpu.s21[i]).norm() < 1e-10, "{serial:?} {cpu:?}");
    }
}
