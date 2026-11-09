//! `results.material`: a sample's εr and μr from the S-parameters of the line it fills, by
//! the Nicolson–Ross–Weir method ([`emcraft_theory::nrw`]).

use emcraft_solver::Complex64 as C;
use serde_json::{Value, json};

use emcraft_theory::nrw::{Sample, extract_sweep};

use crate::network::{port_pair, source};
use crate::{Params, Result, Session};

/// The highest branch of ln(1/T) a reading takes: a sample a thousand guide wavelengths long.
const MAX_BRANCH: usize = 1000;

/// A complex number as a reply gives it.
fn complex(z: C) -> Value {
    json!({"re": z.re, "im": z.im})
}

/// `results.material`.
pub fn material(s: &mut Session, p: &Params) -> Result<Value> {
    let src = source(s, p)?;
    let d = &src.data;
    let (input, output) = port_pair(p, d)?;
    // The empty line's cut-off: given, else the model's guide's TE10 empty when the data is the
    // sweep, else a TEM line's (none).
    let from_sweep = matches!(p.string("data").optional()?, None | Some("sweep"));
    let guide = s.model.waveguide().filter(|_| from_sweep);
    let default_cutoff = guide.map_or(0.0, |w| emcraft_theory::te10_cutoff(w.width.get(), 1.0, 1.0));
    let sample = Sample {
        length: p.positive_length("length").required()?,
        cutoff: p.frequency("cutoff").optional()?.unwrap_or(default_cutoff),
        before: p.length("before").or(0.0)?,
        after: p.length("after").or(0.0)?,
    };
    for (name, x) in [("before", sample.before), ("after", sample.after), ("cutoff", sample.cutoff)] {
        if !(x.is_finite() && x >= 0.0) {
            return Err(p.error(name, "must be ≥ 0"));
        }
    }
    let branch = p.integer("branch", 0..=MAX_BRANCH).or(0)?;
    let branch = i32::try_from(branch).map_err(|_| p.error("branch", "too large"))?;
    let fs = d.frequencies();
    let get = |a: usize, b: usize| (0..fs.len()).map(|k| d.get(k, a, b).unwrap_or_default()).collect::<Vec<C>>();
    let (s11, s21) = (get(input, input), get(output, input));
    let read = extract_sweep(fs, &s11, &s21, &sample, branch);
    let points: Vec<Value> = fs
        .iter()
        .zip(&read)
        .map(|(f, r)| match r {
            Ok(m) => json!({
                "frequency": s.show_frequency(*f),
                "eps_r": complex(m.eps_r),
                "mu_r": complex(m.mu_r),
                "loss_tangent": -m.eps_r.im / m.eps_r.re,
                "magnetic_loss_tangent": -m.mu_r.im / m.mu_r.re,
            }),
            Err(why) => json!({"frequency": s.show_frequency(*f), "why": why}),
        })
        .collect();
    Ok(json!({
        "length": s.show_length(sample.length),
        "cutoff": s.show_frequency(sample.cutoff),
        "ports": [input + 1, output + 1],
        "read": read.iter().filter(|r| r.is_ok()).count(),
        "points": points,
    }))
}
