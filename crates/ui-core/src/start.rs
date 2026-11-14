//! The start page's rules, whatever draws it: a template's model built for its card (without
//! its mesh or solve, its symmetry planes left out of the picture) and seen from the corner, a
//! template's description split into the card's name and line, and the templates' groups'
//! titles. Each window draws the scene on its GPU.

use emcraft_engine::Session;
use emcraft_engine::templates::Group;
use emcraft_render::view3d::{Camera, StandardView};
use serde_json::Value;

use crate::scene3d::{self, Colours, Overlay};
use crate::theme::Theme;

/// A template's picture to draw: a board's preview drawn already, or a scene with the camera to
/// draw it from and the view's ground (top, bottom).
pub enum TemplatePicture {
    Board(emcraft_render::Canvas),
    Scene { scene: emcraft_render::view3d::Scene, camera: Camera, ground: (emcraft_render::Rgb, emcraft_render::Rgb) },
}

/// Template `name`'s model with its parameters changed to `values`, without its mesh or solve,
/// to draw `size` pixels in `theme`; why it cannot be built.
pub fn template_picture(name: &str, values: &Value, size: [usize; 2], theme: &Theme) -> Result<TemplatePicture, String> {
    let steps: Vec<emcraft_engine::Step> = emcraft_engine::template_steps(name, values)
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|s| !["mesh.", "solve.", "results."].iter().any(|p| s.command.starts_with(p)))
        .collect();
    let mut s = Session::new();
    s.run_steps(&steps).map_err(|e| e.to_string())?;
    let t = &theme.tokens;
    if let Some(board) = s.model().pcb() {
        let panel = t.panel.rgb();
        return emcraft_render::pcb::preview(board, size, panel, panel.mix(t.success.rgb(), 0.25), t.warning.rgb()).map(TemplatePicture::Board);
    }
    // The model's shape only: its symmetry planes (how it is solved) left out of the picture.
    for axis in s.model().domain.symmetry.iter().map(|q| q.axis).collect::<Vec<_>>() {
        let plane =
            serde_json::json!({"kind": "geometry", "name": emcraft_engine::visibility::GeometryObject::Symmetry(axis).key(), "visible": false});
        s.execute("view.visibility", &plane).map_err(|e| e.to_string())?;
    }
    let built = scene3d::build(&s, Overlay::Materials, &Colours::of(theme));
    let (lo, hi) = built.bounds.ok_or("nothing to draw")?;
    let [w, h] = size;
    let mut camera = Camera::default();
    camera.standard(StandardView::Iso);
    camera.fit(lo, hi, w as f64 / h.max(1) as f64);
    camera.distance *= 0.8;
    Ok(TemplatePicture::Scene { scene: built.scene, camera, ground: (t.viewport_top.rgb(), t.viewport_bottom.rgb()) })
}

/// A template's description as a card's name and the line under it: split at its first colon,
/// comma, dash or bracket that leaves a name of 6 to 48 characters (`Waveguide high-pass
/// filter` · `WR-90 filled with …`), else whole as the name.
pub fn title_and_rest(about: &str) -> (&str, &str) {
    let mut best: Option<usize> = None;
    for sep in [": ", "：", ", ", "、", " — ", " (", "（"] {
        if let Some(i) = about.find(sep) {
            let n = about[..i].chars().count();
            if (6..=48).contains(&n) && best.is_none_or(|b| i < b) {
                best = Some(i);
            }
        }
    }
    match best {
        Some(i) => (about[..i].trim(), about[i..].trim_start_matches([':', '：', ',', '、', ' ', '—']).trim()),
        None => (about.trim(), ""),
    }
}

/// A group of templates' title.
pub fn group_title(group: Group) -> crate::lang::Text {
    match group {
        Group::Filters => t!("Filters"),
        Group::Lines => t!("Guides and lines"),
        Group::Junctions => t!("Bends, tees and steps"),
        Group::Couplers => t!("Couplers and dividers"),
        Group::Materials => t!("Loads, materials and ferrites"),
        Group::Periodic => t!("Periodic guides and compressors"),
        Group::Resonators => t!("Cavities and resonators"),
        Group::Dispersion => t!("Dispersion"),
        Group::Antennas => t!("Antennas"),
        Group::Scattering => t!("Scattering"),
        Group::Boards => t!("Board checks"),
        Group::Systems => t!("Systems"),
        Group::Particles => t!("Electrons and plasmas"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_description_splits_into_a_name_and_a_line() {
        assert_eq!(super::title_and_rest("Waveguide high-pass filter: WR-90 filled"), ("Waveguide high-pass filter", "WR-90 filled"));
        assert_eq!(super::title_and_rest("Straight rectangular waveguide, WR-90 (a × b)"), ("Straight rectangular waveguide", "WR-90 (a × b)"));
        assert_eq!(super::title_and_rest("Short"), ("Short", ""));
    }
}
