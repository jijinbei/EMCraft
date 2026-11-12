//! A template opened from the start page: a card over the window with the
//! template's parameters on the left (each its words, its name dim under them, its value and
//! unit, red while it is no number) and its model drawn on the right, drawn again off the
//! window's thread as they change (the last good picture kept, dimmed, with why the values make
//! no model); Open builds the project from it (`file.sample` with the values changed and the
//! purpose chosen on the start page), Reset puts the defaults back. The fields are the shared
//! rules' ([`emcraft_ui_core::template_card`]).

use emcraft_ui_core::lang::{parameter_label, sample_label, tr};
use emcraft_ui_core::template_card::TemplateCard;
use iced::widget::{Column, Space, button, center, column, container, image, opaque, row, text, text_input};
use iced::{Alignment, Color, Element, Fill, Task, Widget};

use crate::app::{App, Message};
use crate::look;

/// The preview's size in points and pixels (drawn at twice it).
const PREVIEW: [f32; 2] = [440.0, 290.0];
const PIXELS: [usize; 2] = [880, 580];

/// What the template card asks for.
#[derive(Clone, Debug)]
pub enum TemplateAct {
    /// The card of template `name` opened, or closed.
    Open(Option<&'static str>),
    Field(usize, String),
    Reset,
    /// The project built from it.
    Build,
    /// A preview drawn for the fields' texts: its pixels, or why not.
    Drawn(Vec<String>, Result<(u32, u32, Vec<u8>), String>),
}

fn send(a: TemplateAct) -> Message {
    Message::Template(a)
}

/// The template card open, and its preview: the fields' texts it was drawn for and the picture
/// (the last good one kept).
#[derive(Clone, Debug, Default)]
pub struct Open {
    pub card: Option<TemplateCard>,
    drawn_for: Option<Vec<String>>,
    picture: Option<image::Handle>,
}

impl App {
    /// What the template card asks for, and the preview drawn off the window's thread when its
    /// fields changed.
    pub(crate) fn template_act(&mut self, a: TemplateAct) -> Task<Message> {
        match a {
            TemplateAct::Open(Some(name)) => self.template = Open { card: TemplateCard::new(&self.session, name), ..Open::default() },
            TemplateAct::Open(None) => self.template = Open::default(),
            TemplateAct::Field(i, s) => {
                if let Some(f) = self.template.card.as_mut().and_then(|c| c.fields.get_mut(i)) {
                    f.text = emcraft_ui_core::dialogs::bounded(&s);
                }
            }
            TemplateAct::Reset => self.template.card = self.template.card.take().map(TemplateCard::reset),
            TemplateAct::Build => {
                let Some(card) = self.template.card.as_ref() else { return Task::none() };
                let Ok(values) = card.values() else { return Task::none() };
                let name = card.name.clone();
                let template = emcraft_engine::templates::find(&name);
                let purpose = self.purpose.filter(|p| template.is_some_and(|t| t.kinds.contains(p)));
                self.template = Open::default();
                let then = emcraft_ui_core::files::Then::Sample { name, values, purpose };
                return self.file(emcraft_ui_core::files::FileAction::Request(then));
            }
            TemplateAct::Drawn(texts, drawn) => {
                if self.template.card.as_ref().is_some_and(|c| c.texts() == texts) {
                    match drawn {
                        Ok((w, h, rgba)) => {
                            self.template.picture = Some(image::Handle::from_rgba(w, h, rgba));
                            self.template.card.iter_mut().for_each(|c| c.error = None);
                        }
                        Err(e) => self.template.card.iter_mut().for_each(|c| c.error = Some(e.clone())),
                    }
                }
                return Task::none();
            }
        }
        self.draw_template()
    }

    /// The open card's preview drawn for its fields' texts, when they changed since it was.
    fn draw_template(&mut self) -> Task<Message> {
        let Some(card) = self.template.card.as_mut() else { return Task::none() };
        let texts = card.texts();
        if self.template.drawn_for.as_ref() == Some(&texts) {
            return Task::none();
        }
        self.template.drawn_for = Some(texts.clone());
        let values = match card.values() {
            Ok(v) => v,
            Err(e) => {
                card.error = Some(e);
                return Task::none();
            }
        };
        let (name, theme) = (card.name.clone(), (*self.look.theme).clone());
        let work = async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(crate::start::drawn_with(&name, &values, theme, PIXELS));
            });
            rx.await.unwrap_or_else(|_| Err("the drawing stopped".into()))
        };
        Task::perform(work, move |r| send(TemplateAct::Drawn(texts.clone(), r)))
    }
}

/// The card over the window, while a template is open.
pub fn view(app: &App) -> Option<Element<'_, Message>> {
    let card = app.template.card.as_ref()?;
    let t = app.look.tokens;
    let title = row![
        text(sample_label(&card.name).to_owned()).size(t.font_heading),
        container(text(card.name.clone()).size(t.font_small).color(t.text_dim)).padding([1, 6]).style(look::ground(t))
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    let fields = card.fields.iter().enumerate().map(|(i, f)| {
        let bad = f.value().is_none();
        let words = match &f.label {
            Some(l) => column![
                text(parameter_label(l).to_owned()).size(t.font_body).color(t.form_ink),
                text(f.name.clone()).size(t.font_small).color(t.text_dim)
            ],
            None => column![text(f.name.clone()).size(t.font_body).color(t.form_ink)],
        };
        let input = text_input("", &f.text)
            .size(t.font_body)
            .width(90)
            .padding([3, 6])
            .style(look::field(t))
            .on_input(move |s| send(TemplateAct::Field(i, s)));
        let unit = text(f.unit).size(t.font_small).color(if bad { t.error } else { t.text_dim });
        row![words.width(170), input, unit].spacing(6).align_y(Alignment::Center).boxed()
    });
    let none = card.fields.is_empty().then(|| text(tr(t!("none"))).size(t.font_small).color(t.text_dim).boxed());
    let list = Column::with_children(
        std::iter::once(text(tr(t!("Parameters")).to_uppercase()).size(t.font_small).color(t.text_dim).boxed()).chain(none).chain(fields),
    )
    .spacing(6)
    .width(300);
    let picture: Element<Message> = match &app.template.picture {
        Some(h) => image(h.clone()).width(PREVIEW[0]).height(PREVIEW[1]).opacity(if card.error.is_some() { 0.45_f32 } else { 1.0_f32 }).boxed(),
        None => Space::new().width(PREVIEW[0]).height(PREVIEW[1]).boxed(),
    };
    let error = card.error.as_ref().map(|e| text(e.clone()).size(t.font_small).color(t.error).boxed());
    let ready = card.error.is_none() && card.values().is_ok() && app.job.is_none();
    let buttons = row![
        button(text(tr(t!("Open"))).size(t.font_button)).style(look::button(t, true)).on_press_maybe(ready.then_some(send(TemplateAct::Build))),
        button(text(tr(t!("Cancel"))).size(t.font_button)).style(look::button(t, false)).on_press(send(TemplateAct::Open(None))),
        button(text(tr(t!("Reset"))).size(t.font_button)).style(look::quiet(t)).on_press(send(TemplateAct::Reset))
    ]
    .spacing(8);
    let body = Column::with_children(
        [title.boxed(), row![list, picture].spacing(16).boxed()].into_iter().chain(error).chain(std::iter::once(buttons.boxed())),
    )
    .spacing(12);
    let card = container(body).padding(18).style(look::ground(t));
    let veil = container(center(card))
        .width(Fill)
        .height(Fill)
        .style(|_: &iced::Theme| container::Style { background: Some(Color { a: 0.35, ..Color::BLACK }.into()), ..container::Style::default() });
    Some(opaque(veil).boxed())
}
