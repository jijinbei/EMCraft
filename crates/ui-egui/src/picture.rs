//! CPU pictures shown as textures, and the cache that redraws one only when what it shows
//! changes (its key: the session's revision, the picture's width, the theme, the view's
//! settings — [`PictureKey`]).

use std::sync::Arc;

/// A CPU picture shown as a texture, uploaded to each egui context that draws it (the window's,
/// and the offscreen one of `ui.render`).
pub struct Picture {
    pub image: Arc<egui::ColorImage>,
    textures: Vec<(egui::Context, egui::TextureHandle)>,
    regions: Vec<(egui::Context, [usize; 4], egui::TextureHandle)>,
}

impl Picture {
    pub fn new(canvas: &emcraft_render::Canvas) -> Picture {
        let image = egui::ColorImage::from_rgba_unmultiplied([canvas.w, canvas.h], &canvas.rgba);
        Picture { image: Arc::new(image), textures: Vec::new(), regions: Vec::new() }
    }

    pub fn size(&self) -> egui::Vec2 {
        egui::vec2(self.image.size[0] as f32, self.image.size[1] as f32)
    }

    /// Uploads the visible rectangle `[left, top, right, bottom]` in source pixels.
    /// A scrolled portrait retains full horizontal resolution even if its total height exceeds
    /// the backend's texture limit. Empty/outside rectangles return `None`.
    pub(crate) fn region_texture(&mut self, ctx: &egui::Context, bounds: [usize; 4]) -> Option<egui::TextureId> {
        let [w, h] = self.image.size;
        let [x0, y0, x1, y1] = [bounds[0].min(w), bounds[1].min(h), bounds[2].min(w), bounds[3].min(h)];
        if x0 >= x1 || y0 >= y1 {
            return None;
        }
        let bounds = [x0, y0, x1, y1];
        if let Some((_, _, t)) = self.regions.iter().find(|(c, b, _)| c == ctx && *b == bounds) {
            return Some(t.id());
        }
        let mut pixels = Vec::with_capacity((x1 - x0) * (y1 - y0));
        for y in y0..y1 {
            pixels.extend_from_slice(self.image.pixels.get(y * w + x0..y * w + x1)?);
        }
        let image = egui::ColorImage::new([x1 - x0, y1 - y0], pixels);
        let max = ctx.input(|i| i.max_texture_side).max(1);
        let image = fit_within(&image, max).unwrap_or(image);
        if let Some((_, old, t)) = self.regions.iter_mut().find(|(c, _, _)| c == ctx) {
            *old = bounds;
            t.set(image, egui::TextureOptions::LINEAR);
            return Some(t.id());
        }
        let t = ctx.load_texture("picture:region", image, egui::TextureOptions::LINEAR);
        let id = t.id();
        if self.regions.len() >= 2 {
            drop(self.regions.remove(1));
        }
        self.regions.push((ctx.clone(), bounds, t));
        Some(id)
    }

    /// Its texture in `ctx`.
    pub fn texture(&mut self, ctx: &egui::Context) -> egui::TextureId {
        if let Some((_, h)) = self.textures.iter().find(|(c, _)| c == ctx) {
            return h.id();
        }
        // A texture larger than the backend takes makes egui panic: such a picture goes up
        // shrunk to fit, and is still drawn at its own size.
        let max = ctx.input(|i| i.max_texture_side).max(1);
        let image = match fit_within(&self.image, max) {
            Some(small) => Arc::new(small),
            None => self.image.clone(),
        };
        let h = ctx.load_texture("picture", image, egui::TextureOptions::LINEAR);
        let id = h.id();
        // The window's context and at most one offscreen one.
        if self.textures.len() >= 2 {
            drop(self.textures.remove(1));
        }
        self.textures.push((ctx.clone(), h));
        id
    }
}

/// `image` shrunk so neither side exceeds `max`, each pixel the mean of the pixels it covers;
/// `None` when it already fits.
fn fit_within(image: &egui::ColorImage, max: usize) -> Option<egui::ColorImage> {
    let [w, h] = image.size;
    if w <= max && h <= max {
        return None;
    }
    let scale = (w.max(h) as f64) / (max as f64);
    let (nw, nh) = (((w as f64 / scale).floor() as usize).clamp(1, max), ((h as f64 / scale).floor() as usize).clamp(1, max));
    let mut pixels = Vec::with_capacity(nw * nh);
    for y in 0..nh {
        let (y0, y1) = (y * h / nh, ((y + 1) * h / nh).max(y * h / nh + 1).min(h));
        for x in 0..nw {
            let (x0, x1) = (x * w / nw, ((x + 1) * w / nw).max(x * w / nw + 1).min(w));
            let mut sum = [0u32; 4];
            for yy in y0..y1 {
                for p in image.pixels.get(yy * w + x0..yy * w + x1).unwrap_or(&[]) {
                    for (s, c) in sum.iter_mut().zip(p.to_array()) {
                        *s += u32::from(c);
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)).max(1) as u32;
            let [r, g, b, a] = sum.map(|s| (s / n) as u8);
            pixels.push(egui::Color32::from_rgba_premultiplied(r, g, b, a));
        }
    }
    Some(egui::ColorImage::new([nw, nh], pixels))
}

/// What a view's picture was drawn from: the session's revision, the picture's width, the
/// theme, and the view's own settings (`view`).
#[derive(Clone, Debug, PartialEq)]
pub struct PictureKey<V> {
    pub revision: u64,
    pub width: usize,
    /// The height it was drawn at, where the view decides it (0: from the width).
    pub height: usize,
    pub theme: crate::theme::ThemeKey,
    pub view: V,
}

impl<V> PictureKey<V> {
    /// The key at the session's `revision`, `width` pixels wide, in the current theme.
    pub fn new(revision: u64, width: usize, view: V) -> Self {
        PictureKey { revision, width, height: 0, theme: crate::theme::key(), view }
    }

    /// The key drawn `height` pixels high.
    pub fn with_height(self, height: usize) -> Self {
        PictureKey { height, ..self }
    }
}

/// A picture kept until its key changes: a view's chart or plot, drawn again only when the
/// session, the view's settings, its width or the theme change.
pub struct Cached<K>(Option<(K, Picture)>);

impl<K> Default for Cached<K> {
    fn default() -> Self {
        Cached(None)
    }
}

impl<K: PartialEq> Cached<K> {
    /// The key of the picture held, if any.
    pub fn key(&self) -> Option<&K> {
        self.0.as_ref().map(|(k, _)| k)
    }

    pub fn is_some(&self) -> bool {
        self.0.is_some()
    }

    /// Whether the picture held was drawn for `key`.
    pub fn is_fresh(&self, key: &K) -> bool {
        self.key() == Some(key)
    }

    /// Lets the picture go: the next frame draws it again.
    pub fn clear(&mut self) {
        self.0 = None;
    }

    /// Keeps `canvas` as the picture for `key`.
    pub fn set(&mut self, key: K, canvas: &emcraft_render::Canvas) -> &mut Picture {
        &mut self.0.insert((key, Picture::new(canvas))).1
    }

    pub fn get_mut(&mut self) -> Option<&mut Picture> {
        self.0.as_mut().map(|(_, p)| p)
    }

    /// The picture for `key`: the one held when it was drawn for that key, else `draw`'s. When
    /// `draw` fails, nothing is held and its error comes back.
    pub fn get_or_draw<E: ToString>(&mut self, key: K, draw: impl FnOnce() -> Result<emcraft_render::Canvas, E>) -> Result<&mut Picture, String> {
        if self.is_fresh(&key) {
            return self.get_mut().ok_or_else(|| "no picture".to_owned());
        }
        match draw() {
            Ok(canvas) => Ok(self.set(key, &canvas)),
            Err(e) => {
                self.clear();
                Err(e.to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_taller_than_the_largest_texture_is_shown_without_a_panic() {
        let canvas = emcraft_render::Canvas { w: 900, h: 2474, rgba: vec![200; 900 * 2474 * 4] };
        let mut p = Picture::new(&canvas);
        let ctx = egui::Context::default();
        p.texture(&ctx);
        assert_eq!(p.size(), egui::vec2(900.0, 2474.0));
        let small = fit_within(&p.image, 2048).map(|i| i.size);
        assert_eq!(small, Some([745, 2048]));
        assert!(fit_within(&p.image, 4096).is_none());
        // A scrolled visible region keeps all 900 columns, rather than the whole-image
        // fallback's 745: even a backend capped at 2048 can display this part crisply.
        let id = p.region_texture(&ctx, [0, 2000, 900, 2474]).unwrap();
        let mut delta = ctx.tex_manager().write().take_delta();
        let upload = delta.set.get(&id).unwrap().last().unwrap();
        assert_eq!(upload.image.size(), [900, 474]);
        delta.clear();
        assert_eq!(p.region_texture(&ctx, [0, 2000, 900, 2474]), Some(id));
        let mut unchanged = ctx.tex_manager().write().take_delta();
        assert!(unchanged.set.is_empty());
        unchanged.clear();
        assert!(p.region_texture(&ctx, [900, 0, 1000, 20]).is_none());
        assert!(p.region_texture(&ctx, [0, 50, 10, 40]).is_none());
    }

    #[test]
    fn a_cached_picture_is_drawn_again_only_for_another_key() {
        let canvas = emcraft_render::Canvas { w: 2, h: 2, rgba: vec![0; 16] };
        let mut c: Cached<PictureKey<u8>> = Cached::default();
        let key = |view| PictureKey { revision: 1, width: 400, height: 0, theme: crate::theme::key(), view };
        let mut draws = 0;
        for k in [1, 1, 2, 2, 1] {
            let _ = c.get_or_draw(key(k), || {
                draws += 1;
                Ok::<_, String>(canvas.clone())
            });
        }
        assert_eq!(draws, 3);
        assert!(c.get_or_draw(key(3), || Err("no")).is_err());
        assert!(!c.is_some());
    }
}
