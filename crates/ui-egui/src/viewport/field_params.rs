//! The field view's settings as the parameters of the `view.field` and `view.animation`
//! commands: what the side column's Picture… and Video… open those dialogs with. The engine
//! reads them through `Params` in display units (`emcraft_engine::view::field_view`), so their
//! typed form lives here, on the UI's side: [`FieldParams`] serializes to exactly the JSON
//! the dialogs take, and reads back from it.

use emcraft_engine::Session;
use emcraft_engine::view::{Component, Plane};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::state::FieldState;

emcraft_macros::named_enum! {
    /// A vector field drawn as arrows.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Arrow {
        E = "e",
        H = "h",
    }
}

emcraft_macros::named_serde!(Arrow);

/// `view.field`'s parameters (`view.animation`'s without `phase`, with `frames`), in display
/// units.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FieldParams {
    pub path: String,
    pub plane: Plane,
    pub component: Component,
    pub arrows: Vec<Arrow>,
    /// ωt in degrees, to a tenth; none for an animation, which plays a whole period.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<f64>,
    /// An eigenmode, counted from 1; the solved `frequency` when none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency: Option<f64>,
    /// The plane's position along its normal, to 1e-4; the middle when none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<f64>,
    /// The excited entry of S, named when there is more than one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excitation: Option<String>,
    /// An animation's frames in a period.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frames: Option<usize>,
}

/// Frames in a period of a saved animation.
const FRAMES: usize = 24;

impl FieldParams {
    /// `view.field`'s parameters for the field view `field` of `session`, written to `path`.
    pub fn picture(field: &FieldState, session: &Session, path: &str) -> FieldParams {
        let v = &field.view;
        let entries = session.sparameter_entries();
        let arrows = [(v.show_e, Arrow::E), (v.show_h, Arrow::H)].into_iter().filter(|(on, _)| *on).map(|(_, a)| a).collect();
        FieldParams {
            path: path.to_owned(),
            plane: v.plane,
            component: v.component,
            arrows,
            phase: Some((v.phase.to_degrees() * 10.0).round() / 10.0),
            mode: v.mode.map(|m| m + 1),
            frequency: v.mode.is_none().then(|| session.display().frequency.from_si(v.frequency)),
            at: field.at.map(|at| (at * 1e4).round() / 1e4),
            excitation: entries.get(v.excitation).filter(|_| entries.len() > 1).map(|e| e.name.clone()),
            frames: None,
        }
    }

    /// `view.animation`'s: a period of the same view at [`FRAMES`] frames, written to `path`.
    pub fn animation(field: &FieldState, session: &Session, path: &str) -> FieldParams {
        FieldParams { phase: None, frames: Some(FRAMES), ..FieldParams::picture(field, session, path) }
    }

    /// The command's JSON parameters.
    pub fn to_params(&self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| json!({}))
    }
}

#[cfg(test)]
mod tests {
    use emcraft_engine::Params;
    use emcraft_engine::view::FieldView;

    use super::*;

    /// The JSON the side column wrote by hand before the parameters were typed.
    fn by_hand(field: &FieldState, session: &Session, path: &str) -> Value {
        let v = &field.view;
        let mut arrows = Vec::new();
        if v.show_e {
            arrows.push("e");
        }
        if v.show_h {
            arrows.push("h");
        }
        let mut p = json!({"path": path, "plane": v.plane.id(), "component": v.component.id(), "arrows": arrows,
            "phase": (v.phase.to_degrees() * 10.0).round() / 10.0});
        match v.mode {
            Some(m) => p["mode"] = json!(m + 1),
            None => p["frequency"] = json!(session.display().frequency.from_si(v.frequency)),
        }
        if let Some(at) = field.at {
            p["at"] = json!((at * 1e4).round() / 1e4);
        }
        let entries = session.sparameter_entries();
        if let Some(entry) = entries.get(v.excitation)
            && entries.len() > 1
        {
            p["excitation"] = json!(entry.name);
        }
        p
    }

    fn states() -> Vec<FieldState> {
        let mut out = vec![FieldState::default()];
        let mut f = FieldState::default();
        f.view.plane = Plane::Cross;
        f.view.component = Component::H;
        (f.view.show_e, f.view.show_h) = (true, true);
        f.view.phase = 1.0;
        f.at = Some(12.345678);
        f.view.frequency = 8.25e9;
        out.push(f);
        let mut f = FieldState::default();
        f.view.mode = Some(2);
        (f.view.show_e, f.view.show_h) = (false, false);
        f.view.component = Component::Ez;
        out.push(f);
        out
    }

    #[test]
    fn the_typed_parameters_write_the_json_the_dialogs_took() {
        let s = Session::new();
        for f in states() {
            let p = FieldParams::picture(&f, &s, "field.png");
            assert_eq!(p.to_params(), by_hand(&f, &s, "field.png"));
            // And read back from it.
            assert_eq!(serde_json::from_value::<FieldParams>(p.to_params()).ok(), Some(p));
            let mut video = by_hand(&f, &s, "v.png");
            if let Some(o) = video.as_object_mut() {
                o.remove("phase");
                o.insert("frames".into(), json!(24));
            }
            assert_eq!(FieldParams::animation(&f, &s, "v.png").to_params(), video);
        }
        assert!(serde_json::from_value::<FieldParams>(json!({"path": "x", "plane": "diagonal", "component": "ey", "arrows": []})).is_err());
    }

    #[test]
    fn the_engine_reads_them_as_the_view_they_came_from() {
        let s = Session::new();
        let l = s.display().length;
        for f in states() {
            let json = FieldParams::picture(&f, &s, "field.png").to_params();
            let read: Option<FieldView> = emcraft_engine::view::field_view(&s, &Params::new(&json, s.display())).ok();
            let Some(read) = read else { panic!("refused: {json}") };
            let v = &f.view;
            assert_eq!((read.plane, read.component, read.show_e, read.show_h, read.mode), (v.plane, v.component, v.show_e, v.show_h, v.mode));
            assert!((read.phase - v.phase).abs() < 0.1f64.to_radians());
            if v.mode.is_none() {
                assert!((read.frequency - v.frequency).abs() < 1e-6 * v.frequency);
            }
            assert_eq!(read.at.is_some(), f.at.is_some());
            if let (Some(a), Some(b)) = (read.at, f.at) {
                assert!((a - l.to_si(b)).abs() < l.to_si(1e-4));
            }
        }
    }
}
