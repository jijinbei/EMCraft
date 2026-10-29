//! CPU pictures shown as textures, and the cache that redraws one only when what it shows
//! changes (its key: the session's revision, the picture's width, the theme, the view's
//! settings — [`PictureKey`]).

use std::sync::Arc;

/// A CPU picture shown as a texture, uploaded to each egui context that draws it (the window's,
/// and the offscreen one of `ui.render`).
pub struct Picture {
    pub image: Arc<egui::ColorImage>,
    textures: Vec<(egui::Context, egui::TextureHandle)>,
}

impl Picture {
    pub fn new(canvas: &emcraft_render::Canvas) -> Picture {
        let image = egui::ColorImage::from_rgba_unmultiplied([canvas.w, canvas.h], &canvas.rgba);
        Picture { image: Arc::new(image), textures: Vec::new() }
    }

    pub fn size(&self) -> egui::Vec2 {
        egui::vec2(self.image.size[0] as f32, self.image.size[1] as f32)
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
    pub dark: bool,
    pub view: V,
}

impl<V> PictureKey<V> {
    /// The key at the session's `revision`, `width` pixels wide, in the current theme.
    pub fn new(revision: u64, width: usize, view: V) -> Self {
        PictureKey { revision, width, dark: crate::theme::is_dark(), view }
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
    }

    #[test]
    fn a_cached_picture_is_drawn_again_only_for_another_key() {
        let canvas = emcraft_render::Canvas { w: 2, h: 2, rgba: vec![0; 16] };
        let mut c: Cached<PictureKey<u8>> = Cached::default();
        let key = |view| PictureKey { revision: 1, width: 400, dark: false, view };
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
