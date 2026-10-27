//! CPU pictures shown as textures, and the cache that redraws one only when what it shows
//! changes (its key: the session's revision, the picture's width, the theme, the view's
//! settings).

use std::sync::Arc;

/// A CPU picture shown as a texture, uploaded to each egui context that draws it (the window's,
/// and the offscreen one of `ui.render`).
pub struct Picture {
    pub key: String,
    pub image: Arc<egui::ColorImage>,
    textures: Vec<(egui::Context, egui::TextureHandle)>,
}

impl Picture {
    pub fn new(key: String, canvas: &emcraft_render::Canvas) -> Picture {
        let image = egui::ColorImage::from_rgba_unmultiplied([canvas.w, canvas.h], &canvas.rgba);
        Picture { key, image: Arc::new(image), textures: Vec::new() }
    }

    pub fn size(&self) -> egui::Vec2 {
        egui::vec2(self.image.size[0] as f32, self.image.size[1] as f32)
    }

    /// Its texture in `ctx`.
    pub fn texture(&mut self, ctx: &egui::Context) -> egui::TextureId {
        if let Some((_, h)) = self.textures.iter().find(|(c, _)| c == ctx) {
            return h.id();
        }
        let h = ctx.load_texture(format!("picture:{}", self.key.len()), self.image.clone(), egui::TextureOptions::LINEAR);
        let id = h.id();
        // The window's context and at most one offscreen one.
        if self.textures.len() >= 2 {
            drop(self.textures.remove(1));
        }
        self.textures.push((ctx.clone(), h));
        id
    }
}

/// A picture kept until its key changes: a view's chart or plot, drawn again only when the
/// session, the view's settings, its width or the theme change.
#[derive(Default)]
pub struct Cached(Option<Picture>);

impl Cached {
    /// The key of the picture held, if any.
    pub fn key(&self) -> Option<&str> {
        self.0.as_ref().map(|p| p.key.as_str())
    }

    pub fn is_some(&self) -> bool {
        self.0.is_some()
    }

    /// Whether the picture held was drawn for `key`.
    pub fn is_fresh(&self, key: &str) -> bool {
        self.key() == Some(key)
    }

    /// Lets the picture go: the next frame draws it again.
    pub fn clear(&mut self) {
        self.0 = None;
    }

    /// Keeps `canvas` as the picture for `key`.
    pub fn set(&mut self, key: String, canvas: &emcraft_render::Canvas) -> &mut Picture {
        self.0.insert(Picture::new(key, canvas))
    }

    pub fn get_mut(&mut self) -> Option<&mut Picture> {
        self.0.as_mut()
    }

    /// The picture for `key`: the one held when it was drawn for that key, else `draw`'s. When
    /// `draw` fails, nothing is held and its error comes back.
    pub fn get_or_draw<E: ToString>(
        &mut self,
        key: String,
        draw: impl FnOnce() -> Result<emcraft_render::Canvas, E>,
    ) -> Result<&mut Picture, String> {
        if self.is_fresh(&key) {
            return self.0.as_mut().ok_or_else(|| "no picture".to_owned());
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
