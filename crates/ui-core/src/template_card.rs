//! A template's card as the window fills it: its parameters' fields, the one pointed at and why
//! the model cannot be built from them. What the card shows of the model (its preview) is each
//! window's own.

use emcraft_engine::Session;
use serde_json::{Map, Value, json};

use crate::dialogs::short_number;

/// A parameter's field: its name, its text, its unit and the template's own value.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub name: String,
    /// What it is, in words (English: [`crate::lang::parameter_label`] shows it).
    pub label: Option<String>,
    pub text: String,
    pub unit: &'static str,
    pub default: f64,
}

impl Field {
    /// Its text as a value: `None` unless a finite number.
    pub fn value(&self) -> Option<f64> {
        self.text.trim().parse::<f64>().ok().filter(|v| v.is_finite())
    }
}

/// The open template's card.
#[derive(Clone, Debug, PartialEq)]
pub struct TemplateCard {
    pub name: String,
    pub fields: Vec<Field>,
    /// The parameter pointed at (its field focused or under the pointer): its mark is drawn.
    pub active: Option<String>,
    /// Why the model cannot be built from the fields as they stand.
    pub error: Option<String>,
}

impl TemplateCard {
    /// Template `name` with its own values, in `session`'s display units (`None`: no such
    /// template).
    ///
    /// ```
    /// use emcraft_ui_core::template_card::TemplateCard;
    /// let s = emcraft_engine::Session::new();
    /// let card = TemplateCard::new(&s, "highpass").unwrap_or_else(|| panic!("a template"));
    /// assert_eq!(card.values(), Ok(serde_json::json!({})));
    /// assert!(TemplateCard::new(&s, "no such").is_none());
    /// ```
    pub fn new(session: &Session, name: &str) -> Option<TemplateCard> {
        let d = session.display();
        let fields = emcraft_engine::template_parameters(name)?
            .into_iter()
            .map(|p| Field {
                unit: match p.quantity.as_deref() {
                    Some("length") => d.length.symbol(),
                    Some("frequency") => d.frequency.symbol(),
                    _ => "",
                },
                text: short_number(p.value),
                default: p.value,
                label: p.label,
                name: p.name,
            })
            .collect();
        Some(TemplateCard { name: name.to_owned(), fields, active: None, error: None })
    }

    /// The values changed from the template's, by name; an error naming a field that is not a
    /// number.
    pub fn values(&self) -> Result<Value, String> {
        self.fields
            .iter()
            .filter_map(|f| match f.value() {
                None => Some(Err(format!("{}: not a number", f.name))),
                Some(v) => (v != f.default).then(|| Ok((f.name.clone(), json!(v)))),
            })
            .collect::<Result<Map<String, Value>, String>>()
            .map(Value::Object)
    }

    /// The fields' texts, as they stand.
    pub fn texts(&self) -> Vec<String> {
        self.fields.iter().map(|f| f.text.clone()).collect()
    }

    /// The card with every field back at the template's own value.
    pub fn reset(self) -> TemplateCard {
        let fields = self.fields.into_iter().map(|f| Field { text: short_number(f.default), ..f }).collect();
        TemplateCard { fields, ..self }
    }

    /// The card as the control channel reports it.
    pub fn inspect(&self) -> Value {
        let fields: Map<String, Value> = self.fields.iter().map(|f| (f.name.clone(), json!(f.text))).collect();
        json!({"name": self.name, "fields": fields, "error": self.error, "active": self.active})
    }
}
