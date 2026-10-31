//! Standard rectangular waveguides and their recommended bands: the EIA designations (WR-n, the
//! broad wall n/100 inch wide) with the band each is used over, about 1.25 to 1.9 times its TE10
//! cut-off, as the public waveguide tables give them (EIA RS-261-B; IEC 60153-2 lists the same
//! guides as R-numbers). The project's Home offers a port's band as a preset.

use emcraft_model::units::C0;

use crate::Session;

/// A band a rectangular port is made for: a standard guide's (`WR-90`) or, for a guide of no
/// standard width, its TE10 band (1.25 to 1.9 times the cut-off). Hz.
#[derive(Clone, Debug, PartialEq)]
pub struct GuideBand {
    pub name: String,
    pub start: f64,
    pub stop: f64,
}

/// The standard guides: name, broad wall (inch), band (MHz).
const GUIDES: [(&str, f64, u32, u32); 22] = [
    ("WR-650", 6.500, 1120, 1700),
    ("WR-510", 5.100, 1450, 2200),
    ("WR-430", 4.300, 1700, 2600),
    ("WR-340", 3.400, 2200, 3300),
    ("WR-284", 2.840, 2600, 3950),
    ("WR-229", 2.290, 3300, 4900),
    ("WR-187", 1.872, 3950, 5850),
    ("WR-159", 1.590, 4900, 7050),
    ("WR-137", 1.372, 5850, 8200),
    ("WR-112", 1.122, 7050, 10000),
    ("WR-90", 0.900, 8200, 12400),
    ("WR-75", 0.750, 10000, 15000),
    ("WR-62", 0.622, 12400, 18000),
    ("WR-51", 0.510, 15000, 22000),
    ("WR-42", 0.420, 18000, 26500),
    ("WR-34", 0.340, 22000, 33000),
    ("WR-28", 0.280, 26500, 40000),
    ("WR-22", 0.224, 33000, 50000),
    ("WR-19", 0.188, 40000, 60000),
    ("WR-15", 0.148, 50000, 75000),
    ("WR-12", 0.122, 60000, 90000),
    ("WR-10", 0.100, 75000, 110000),
];

const INCH: f64 = 0.0254;

/// The standard air-filled guide whose broad wall is `width` metres (within 1 %), with its band.
///
/// ```
/// use emcraft_engine::guides::standard;
/// let wr90 = standard(22.86e-3).unwrap();
/// assert_eq!((wr90.name.as_str(), wr90.start, wr90.stop), ("WR-90", 8.2e9, 12.4e9));
/// // No standard guide is 30 mm wide; nonsense widths are none either.
/// assert!(standard(30e-3).is_none() && standard(f64::NAN).is_none() && standard(-1.0).is_none());
/// ```
pub fn standard(width: f64) -> Option<GuideBand> {
    if !(width.is_finite() && width > 0.0) {
        return None;
    }
    GUIDES.iter().find(|(_, a, _, _)| (width - a * INCH).abs() <= 0.01 * a * INCH).map(|&(name, _, start, stop)| GuideBand {
        name: name.to_owned(),
        start: f64::from(start) * 1e6,
        stop: f64::from(stop) * 1e6,
    })
}

/// The TE10 band of a rectangular guide `width` metres wide filled with refractive index `n`:
/// 1.25 to 1.9 times its cut-off, rounded to three figures (Hz); none for a nonsense guide.
///
/// ```
/// use emcraft_engine::guides::te10_band;
/// // 30 mm of air: the cut-off is 5.00 GHz.
/// let b = te10_band(30e-3, 1.0).unwrap();
/// assert_eq!((b.start, b.stop), (6.25e9, 9.49e9));
/// assert!(te10_band(0.0, 1.0).is_none() && te10_band(1e-3, f64::INFINITY).is_none());
/// ```
pub fn te10_band(width: f64, n: f64) -> Option<GuideBand> {
    let fc = C0 / (2.0 * width * n);
    if !(fc.is_finite() && fc > 0.0) {
        return None;
    }
    Some(GuideBand { name: "TE10".to_owned(), start: figures(1.25 * fc), stop: figures(1.9 * fc) })
}

/// `x` rounded to three significant figures.
fn figures(x: f64) -> f64 {
    let scale = 10f64.powf(x.log10().floor() - 2.0);
    (x / scale).round() * scale
}

/// The bands the model's rectangular ports are made for (the guide's own when it has no ports):
/// an air-filled port of a standard width gives that guide's band, any other its TE10 band. One
/// per name, in the ports' order.
pub fn bands(s: &Session) -> Vec<GuideBand> {
    let m = &s.model;
    let mut widths: Vec<(f64, f64)> = Vec::new();
    for p in m.ports.iter().take(64) {
        let (Ok(emcraft_model::PortShape::Rect { a, .. }), Ok(mat)) = (m.port_shape_of(p), m.port_material(p.end)) else { continue };
        widths.push((a, (mat.eps_r.get() * mat.mu_r.get()).sqrt()));
    }
    if widths.is_empty()
        && let Some(w) = &m.waveguide
    {
        widths.push((w.width.get(), 1.0));
    }
    let mut out: Vec<GuideBand> = Vec::new();
    for (a, n) in widths {
        let band = if (n - 1.0).abs() < 1e-9 { standard(a).or_else(|| te10_band(a, n)) } else { te10_band(a, n) };
        if let Some(b) = band.filter(|b| !out.iter().any(|o| o.name == b.name)) {
            out.push(b);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wr90_guide_offers_its_band_and_another_its_te10_band() {
        let mut s = Session::new();
        assert!(bands(&s).is_empty());
        let guide = |w: f64| serde_json::json!({"width": w, "height": 10.16, "sections": [{"length": 30, "material": "vacuum"}]});
        s.execute("geometry.waveguide", &guide(22.86)).unwrap();
        assert_eq!(bands(&s), vec![GuideBand { name: "WR-90".into(), start: 8.2e9, stop: 12.4e9 }]);
        s.execute("model.port.create", &serde_json::json!({"name": "1", "end": "start"})).unwrap();
        assert_eq!(bands(&s).len(), 1);
        s.execute("geometry.waveguide", &guide(30.0)).unwrap();
        let b = bands(&s);
        assert_eq!((b[0].name.as_str(), b[0].start), ("TE10", 6.25e9));
        // Every standard guide's band lies within its single-mode range.
        for (name, a, start, stop) in GUIDES {
            let fc = C0 / (2.0 * a * INCH) / 1e6;
            assert!(f64::from(start) > 1.2 * fc && f64::from(stop) < 2.0 * fc, "{name}");
        }
    }
}
