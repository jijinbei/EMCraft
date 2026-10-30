//! Reading command parameters: input-derived numbers are hostile, so every value is checked
//! (finite, in range) and converted from display units to SI here.
//!
//! [`Params`] wraps a command's JSON object; each lookup gives an [`Arg`], which says what to do
//! when the parameter is absent:
//!
//! ```
//! use emcraft_engine::Params;
//! use emcraft_engine::expr::Variables;
//! use emcraft_model::End;
//! use emcraft_model::units::Display;
//! use serde_json::json;
//!
//! let given = json!({"width": "=w/2", "end": "start"});
//! let vars = Variables::from([("w".to_owned(), 22.86)]);
//! let p = Params::new(&given, Display::default()).with_variables(&vars);
//!
//! let width = p.positive_length("width").required()?; // millimetres in, metres out
//! let mu_r = p.number("mu_r").or(1.0)?; // absent: the default
//! let end: End = p.pick("end").required()?;
//! assert!((width - 0.01143).abs() < 1e-15);
//! assert_eq!((mu_r, end), (1.0, End::Start));
//!
//! // Absent, out of range or of the wrong kind: an error naming the parameter.
//! assert!(p.number("height").required().is_err());
//! assert!(p.integer("width", 1..=3).required().is_err());
//! # Ok::<(), emcraft_engine::EngineError>(())
//! ```
//!
//! Every number may also be given as an expression of the session's variables, a string
//! starting with `=` (`"=w/2 + 1"`, [`crate::expr`]): it is evaluated here, before the unit is
//! applied, so it reads as the literal it stands for.

use std::fmt::Display as Show;
use std::ops::RangeInclusive;

use emcraft_fem::Order;
use emcraft_model::units::Display;
use emcraft_model::{Point3, Vec3};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::expr::{self, Variables};
use crate::{EngineError, Result};
use emcraft_macros::Named;

/// The parameters of a command, or of one item of a list inside them.
#[derive(Clone, Debug)]
pub struct Params<'a> {
    value: &'a Value,
    display: Display,
    /// Which item of a list these are ("section 2"), for errors.
    context: Option<String>,
    /// The variables expressions read.
    vars: Option<&'a Variables>,
}

/// One parameter looked up: its value when given, or why it is wrong.
#[must_use = "say what an absent parameter means: required, optional or a default"]
pub struct Arg<'p, T> {
    params: &'p Params<'p>,
    name: &'p str,
    value: Result<Option<T>>,
}

impl<'a> Params<'a> {
    pub fn new(value: &'a Value, display: Display) -> Params<'a> {
        Params { value, display, context: None, vars: None }
    }

    /// The parameters with `vars` for the expressions among them.
    pub fn with_variables(self, vars: &'a Variables) -> Params<'a> {
        Params { vars: Some(vars), ..self }
    }

    /// Other parameters (an object nested in these) read with the same units and variables.
    pub fn nested(&self, value: &'a Value, context: impl Into<String>) -> Params<'a> {
        Params { value, display: self.display, context: Some(context.into()), vars: self.vars }
    }

    /// A JSON value of parameter `name` as a finite number: a number, or an expression of the
    /// variables (`"=w/2"`).
    pub fn number_of(&self, name: &str, v: &Value) -> Result<f64> {
        match v {
            Value::String(text) => {
                let body = expr::body(text).ok_or_else(|| self.error(name, "must be a finite number (or an expression: \"=w/2\")"))?;
                let vars = self.vars.ok_or_else(|| self.error(name, "cannot hold an expression here"))?;
                expr::eval(body, vars).map_err(|e| self.error(name, format!("`{}`: {e}", crate::text::clip(text, 64))))
            }
            _ => finite(v).ok_or_else(|| self.error(name, "must be a finite number")),
        }
    }

    /// An error about parameter `name`.
    pub fn error(&self, name: &str, why: impl Show) -> EngineError {
        match &self.context {
            Some(c) => EngineError::Params(format!("{c}: `{name}` {why}")),
            None => EngineError::Params(format!("`{name}` {why}")),
        }
    }

    /// The parameters' JSON object as given.
    pub fn value(&self) -> &'a Value {
        self.value
    }

    /// The JSON value of `name`, absent when missing or null.
    pub fn raw(&self, name: &str) -> Option<&'a Value> {
        self.value.get(name).filter(|v| !v.is_null())
    }

    pub fn has(&self, name: &str) -> bool {
        self.raw(name).is_some()
    }

    fn arg<'p, T>(&'p self, name: &'p str, read: impl FnOnce(&'a Value) -> Result<T>) -> Arg<'p, T> {
        Arg { params: self, name, value: self.raw(name).map(read).transpose() }
    }

    /// A finite number.
    pub fn number<'p>(&'p self, name: &'p str) -> Arg<'p, f64> {
        self.arg(name, |v| self.number_of(name, v))
    }

    /// A finite number > 0.
    pub fn positive<'p>(&'p self, name: &'p str) -> Arg<'p, f64> {
        self.number(name).and_then(|x| if x > 0.0 { Ok(x) } else { Err(self.error(name, format!("must be > 0, got {x}"))) })
    }

    /// A whole number in `range`.
    pub fn integer<'p>(&'p self, name: &'p str, range: RangeInclusive<usize>) -> Arg<'p, usize> {
        self.arg(name, |v| {
            let n = match v {
                Value::String(_) => {
                    let x = self.number_of(name, v)?;
                    // Whole numbers only, at most 2^53 (exact in f64).
                    (x >= 0.0 && x.fract() == 0.0 && x <= 9.007_199_254_740_992e15).then_some(x as u64)
                }
                _ => v.as_u64(),
            };
            let n = n.and_then(|n| usize::try_from(n).ok()).ok_or_else(|| self.error(name, "must be a whole number ≥ 0"))?;
            if range.contains(&n) { Ok(n) } else { Err(self.error(name, format!("must be {} to {}, got {n}", range.start(), range.end()))) }
        })
    }

    pub fn string<'p>(&'p self, name: &'p str) -> Arg<'p, &'a str> {
        self.arg(name, |v| v.as_str().ok_or_else(|| self.error(name, "must be a string")))
    }

    pub fn flag<'p>(&'p self, name: &'p str) -> Arg<'p, bool> {
        self.arg(name, |v| v.as_bool().ok_or_else(|| self.error(name, format!("must be true or false, got {v}"))))
    }

    /// One of the names a type deserializes from (an enum's variants: `"start"`, `"end"`).
    pub fn choice<'p, T: DeserializeOwned>(&'p self, name: &'p str) -> Arg<'p, T> {
        self.arg(name, |v| serde_json::from_value(v.clone()).map_err(|e| self.error(name, format!("is not one of the choices: {e}"))))
    }

    /// One of a [`Named`] enum's ids.
    pub fn pick<'p, T: Named>(&'p self, name: &'p str) -> Arg<'p, T> {
        self.arg(name, |v| v.as_str().and_then(T::parse).ok_or_else(|| self.error(name, format!("must be {}", T::listed()))))
    }

    /// A list of at most `max` values.
    pub fn list<'p>(&'p self, name: &'p str, max: usize) -> Arg<'p, &'a [Value]> {
        self.arg(name, |v| {
            let list = v.as_array().ok_or_else(|| self.error(name, "must be an array"))?;
            if list.len() > max {
                return Err(self.error(name, format!("holds at most {max} items, got {}", list.len())));
            }
            Ok(list.as_slice())
        })
    }

    /// A list of at most `max` objects, each read as parameters of its own (`what` 1, 2, … in
    /// errors).
    pub fn items(&self, name: &str, what: &str, max: usize) -> Result<Vec<Params<'a>>> {
        let list = self.list(name, max).or(&[])?;
        Ok(list.iter().enumerate().map(|(i, value)| self.nested(value, format!("{what} {}", i + 1))).collect())
    }

    /// Three finite numbers `[x, y, z]`.
    fn triple(&self, name: &str, v: &Value) -> Result<[f64; 3]> {
        let list = v.as_array().filter(|a| a.len() == 3).ok_or_else(|| self.error(name, "must be [x, y, z]"))?;
        let c = |v: &Value| self.number_of(name, v);
        Ok([c(&list[0])?, c(&list[1])?, c(&list[2])?])
    }

    /// A direction or other plain vector `[x, y, z]`.
    pub fn vector<'p>(&'p self, name: &'p str) -> Arg<'p, Vec3> {
        self.arg(name, |v| self.triple(name, v).map(|[x, y, z]| Vec3::new(x, y, z)))
    }

    /// Finite numbers in display units of length, in metres.
    pub fn lengths<'p>(&'p self, name: &'p str) -> Arg<'p, Vec<f64>> {
        self.arg(name, |v| {
            let list = v.as_array().ok_or_else(|| self.error(name, "must be an array of numbers"))?;
            list.iter().map(|x| self.number_of(name, x).map(|x| self.display.length.to_si(x))).collect()
        })
    }

    /// A point `[x, y, z]` of lengths in display units, in metres.
    pub fn point<'p>(&'p self, name: &'p str) -> Arg<'p, Point3> {
        self.arg(name, |v| self.point_of(name, v))
    }

    /// A JSON `[x, y, z]` of lengths in display units, in metres.
    pub fn point_of(&self, name: &str, v: &Value) -> Result<Point3> {
        let [x, y, z] = self.triple(name, v)?.map(|c| self.display.length.to_si(c));
        Ok(Point3::new(x, y, z))
    }

    /// A length in display units, in metres.
    pub fn length<'p>(&'p self, name: &'p str) -> Arg<'p, f64> {
        let unit = self.display.length;
        self.number(name).map(|v| unit.to_si(v))
    }

    /// A positive length in metres.
    pub fn positive_length<'p>(&'p self, name: &'p str) -> Arg<'p, f64> {
        let unit = self.display.length;
        self.positive(name).map(|v| unit.to_si(v))
    }

    /// A positive frequency in hertz.
    pub fn frequency<'p>(&'p self, name: &'p str) -> Arg<'p, f64> {
        let unit = self.display.frequency;
        self.positive(name).map(|v| unit.to_si(v))
    }

    /// The element order `order`, 1 or 2 (default 2).
    pub fn order(&self) -> Result<Order> {
        self.integer("order", 1..=2).map(|n| if n == 1 { Order::One } else { Order::Two }).or(Order::Two)
    }

    /// A picture's side `name` in pixels, between 200 and `max`.
    pub fn side(&self, name: &str, max: usize, default: usize) -> Result<usize> {
        self.integer(name, 200..=max).or(default)
    }
}

impl<'p, T> Arg<'p, T> {
    /// The value; an error when it is absent.
    pub fn required(self) -> Result<T> {
        self.value?.ok_or_else(|| self.params.error(self.name, "is required"))
    }

    /// The value; an error naming `hint` (what else would have done) when it is absent.
    pub fn required_or(self, hint: &str) -> Result<T> {
        self.value?.ok_or_else(|| self.params.error(self.name, format!("is required ({hint})")))
    }

    pub fn optional(self) -> Result<Option<T>> {
        self.value
    }

    /// The value, or `default` when absent.
    pub fn or(self, default: T) -> Result<T> {
        Ok(self.value?.unwrap_or(default))
    }

    /// The value, or the type's default when absent.
    pub fn or_default(self) -> Result<T>
    where
        T: Default,
    {
        Ok(self.value?.unwrap_or_default())
    }

    /// The value, or `fallback` (when it holds one) when absent.
    pub fn or_maybe(self, fallback: Option<T>) -> Arg<'p, T> {
        Arg { value: self.value.map(|v| v.or(fallback)), ..self }
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Arg<'p, U> {
        Arg { params: self.params, name: self.name, value: self.value.map(|v| v.map(f)) }
    }

    /// A further check of a value given.
    pub fn and_then<U>(self, f: impl FnOnce(T) -> Result<U>) -> Arg<'p, U> {
        Arg { params: self.params, name: self.name, value: self.value.and_then(|v| v.map(f).transpose()) }
    }

    /// Refuses a value given that fails `ok`, saying it `must` be something.
    pub fn check(self, ok: impl FnOnce(&T) -> bool, must: &str) -> Arg<'p, T> {
        let (params, name) = (self.params, self.name);
        self.and_then(|v| if ok(&v) { Ok(v) } else { Err(params.error(name, format!("must be {must}"))) })
    }
}

fn finite(v: &Value) -> Option<f64> {
    v.as_f64().filter(|x| x.is_finite())
}

#[cfg(test)]
mod tests {
    use emcraft_model::End;
    use serde_json::json;

    use super::*;

    #[test]
    fn absent_values_are_required_optional_or_defaulted() {
        let v = json!({"a": 2.0, "n": null});
        let p = Params::new(&v, Display::default());
        assert_eq!(p.number("a").required(), Ok(2.0));
        assert_eq!(p.number("n").optional(), Ok(None));
        assert_eq!(p.number("b").or(7.0), Ok(7.0));
        assert_eq!(p.number("b").or_maybe(Some(3.0)).required(), Ok(3.0));
        assert!(matches!(p.number("b").required(), Err(EngineError::Params(m)) if m == "`b` is required"));
    }

    #[test]
    fn values_are_checked_and_converted_to_si() {
        let v = json!({"w": 2.5, "neg": -1, "end": "start", "bad": "middle", "at": [1, 2, 3], "f": 9, "k": 3});
        let p = Params::new(&v, Display::default());
        assert_eq!(p.positive_length("w").required(), Ok(0.0025));
        assert!(p.positive("neg").required().is_err());
        assert_eq!(p.choice::<End>("end").required(), Ok(End::Start));
        assert!(p.choice::<End>("bad").required().is_err());
        assert_eq!(p.point("at").required(), Ok(Point3::new(0.001, 0.002, 0.003)));
        assert_eq!(p.frequency("f").required(), Ok(9e9));
        assert!(p.integer("k", 1..=2).optional().is_err());
        assert!(p.string("w").optional().is_err());
    }

    #[test]
    fn errors_inside_a_list_name_the_item() {
        let v = json!({"sections": [{"length": 1}, {"length": -1}]});
        let p = Params::new(&v, Display::default());
        let items = p.items("sections", "section", 10).unwrap();
        assert!(items[0].positive("length").required().is_ok());
        let e = items[1].positive("length").required().unwrap_err();
        assert_eq!(e, EngineError::Params("section 2: `length` must be > 0, got -1".into()));
        assert!(p.items("sections", "section", 1).is_err());
    }
}
