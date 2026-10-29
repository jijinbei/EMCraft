//! Expressions of named variables in command parameters (`project.variable`).
//!
//! A number parameter may be given as a string starting with `=`: `"=w"`, `"=w/2 + 1"`,
//! `"=2*(a - b)^2"`, `"=sqrt(er)*pi"`. The grammar is
//!
//! ```text
//! sum     = product (("+" | "-") product)*
//! product = power (("*" | "/") power)*
//! power   = unary ("^" power)?          (right-associative)
//! unary   = ("-" | "+") unary | atom
//! atom    = number | name | name "(" sum ")" | "(" sum ")"
//! ```
//!
//! with the names of the session's variables, the constant `pi` and the functions `sqrt`, `abs`,
//! `sin`, `cos`, `tan` (radians), `exp` and `ln`. The value is a plain number: the parameter's
//! display unit applies to it as to a literal. The parser is recursive descent, bounded in the
//! text's length and the nesting depth, and every result is checked finite.
//!
//! ```
//! use emcraft_engine::expr::{self, Variables};
//!
//! let vars = Variables::from([("w".to_owned(), 4.0), ("er".to_owned(), 9.0)]);
//! let body = expr::body("=w/2 + sqrt(er)").unwrap_or_default();
//! assert_eq!(expr::eval(body, &vars), Ok(5.0));
//! assert_eq!(expr::names(body), Ok(vec!["w".to_owned(), "er".to_owned()]));
//!
//! // An unknown name, a value that is not finite: errors, never a panic.
//! assert!(expr::eval("h + 1", &vars).is_err());
//! assert!(expr::eval("1/(w - 4)", &vars).is_err());
//! assert!(!expr::valid_name("pi"));
//! ```

use std::collections::BTreeMap;

/// The session's variables by name: plain numbers.
pub type Variables = BTreeMap<String, f64>;

/// The longest expression read.
pub const MAX_LEN: usize = 256;
/// The deepest nesting of parentheses, signs and powers.
const MAX_DEPTH: usize = 32;
/// The longest variable name.
pub const MAX_NAME: usize = 32;

/// The expression in `text` (`=` and what follows), if it is one.
pub fn body(text: &str) -> Option<&str> {
    text.strip_prefix('=')
}

/// Whether `name` is a valid variable name: a letter or `_`, then letters, digits or `_`; not
/// a function's or a constant's name.
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    first_ok && name.len() <= MAX_NAME && chars.all(|c| c.is_ascii_alphanumeric() || c == '_') && !RESERVED.contains(&name)
}

const RESERVED: [&str; 8] = ["pi", "sqrt", "abs", "sin", "cos", "tan", "exp", "ln"];

/// The value of the expression `text` (without its `=`) with `vars`.
pub fn eval(text: &str, vars: &Variables) -> Result<f64, String> {
    let mut p = Parser::new(text, Some(vars))?;
    let v = p.sum(0)?;
    p.end()?;
    finite(v)
}

/// The variable names `text` (without its `=`) refers to, in order of appearance; an error
/// when it is not an expression.
pub fn names(text: &str) -> Result<Vec<String>, String> {
    let mut p = Parser::new(text, None)?;
    p.sum(0)?;
    p.end()?;
    Ok(p.seen)
}

fn finite(v: f64) -> Result<f64, String> {
    if v.is_finite() { Ok(v) } else { Err("its value is not a finite number".into()) }
}

struct Parser<'a> {
    text: &'a [u8],
    at: usize,
    /// The variables to read; `None` only collects the names.
    vars: Option<&'a Variables>,
    seen: Vec<String>,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str, vars: Option<&'a Variables>) -> Result<Parser<'a>, String> {
        if text.len() > MAX_LEN {
            return Err(format!("the expression is longer than {MAX_LEN} characters"));
        }
        if !text.is_ascii() {
            return Err("the expression may hold only ASCII characters".into());
        }
        Ok(Parser { text: text.as_bytes(), at: 0, vars, seen: Vec::new() })
    }

    fn skip(&mut self) {
        while self.text.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip();
        self.text.get(self.at).copied()
    }

    fn eat(&mut self, c: u8) -> bool {
        if self.peek() == Some(c) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn end(&mut self) -> Result<(), String> {
        match self.peek() {
            None => Ok(()),
            Some(c) => Err(format!("unexpected `{}` at {}", c as char, self.at + 1)),
        }
    }

    fn deeper(depth: usize) -> Result<usize, String> {
        if depth >= MAX_DEPTH { Err(format!("the expression nests deeper than {MAX_DEPTH}")) } else { Ok(depth + 1) }
    }

    fn sum(&mut self, depth: usize) -> Result<f64, String> {
        let depth = Self::deeper(depth)?;
        let mut v = self.product(depth)?;
        loop {
            if self.eat(b'+') {
                v += self.product(depth)?;
            } else if self.eat(b'-') {
                v -= self.product(depth)?;
            } else {
                return Ok(v);
            }
        }
    }

    fn product(&mut self, depth: usize) -> Result<f64, String> {
        let mut v = self.power(depth)?;
        loop {
            if self.eat(b'*') {
                v *= self.power(depth)?;
            } else if self.eat(b'/') {
                let d = self.power(depth)?;
                if d == 0.0 && self.vars.is_some() {
                    return Err("division by zero".into());
                }
                v /= d;
            } else {
                return Ok(v);
            }
        }
    }

    fn power(&mut self, depth: usize) -> Result<f64, String> {
        let base = self.unary(depth)?;
        if self.eat(b'^') {
            let depth = Self::deeper(depth)?;
            let exponent = self.power(depth)?;
            return Ok(base.powf(exponent));
        }
        Ok(base)
    }

    fn unary(&mut self, depth: usize) -> Result<f64, String> {
        let depth = Self::deeper(depth)?;
        if self.eat(b'-') {
            return Ok(-self.unary(depth)?);
        }
        if self.eat(b'+') {
            return self.unary(depth);
        }
        self.atom(depth)
    }

    fn atom(&mut self, depth: usize) -> Result<f64, String> {
        match self.peek() {
            Some(b'(') => {
                self.at += 1;
                let v = self.sum(depth)?;
                if !self.eat(b')') {
                    return Err("a `(` is not closed".into());
                }
                Ok(v)
            }
            Some(c) if c.is_ascii_digit() || c == b'.' => self.number(),
            Some(c) if c.is_ascii_alphabetic() || c == b'_' => self.name(depth),
            Some(c) => Err(format!("unexpected `{}` at {}", c as char, self.at + 1)),
            None => Err("the expression ends too soon".into()),
        }
    }

    fn number(&mut self) -> Result<f64, String> {
        let start = self.at;
        let digits = |p: &mut Parser| {
            while p.text.get(p.at).is_some_and(u8::is_ascii_digit) {
                p.at += 1;
            }
        };
        digits(self);
        if self.text.get(self.at) == Some(&b'.') {
            self.at += 1;
            digits(self);
        }
        if matches!(self.text.get(self.at), Some(b'e' | b'E')) {
            let mark = self.at;
            self.at += 1;
            if matches!(self.text.get(self.at), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if self.text.get(self.at).is_some_and(u8::is_ascii_digit) {
                digits(self);
            } else {
                self.at = mark;
            }
        }
        let s = std::str::from_utf8(&self.text[start..self.at]).map_err(|e| e.to_string())?;
        s.parse::<f64>().map_err(|_| format!("`{s}` is not a number"))
    }

    fn name(&mut self, depth: usize) -> Result<f64, String> {
        let start = self.at;
        while self.text.get(self.at).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_') {
            self.at += 1;
        }
        let name = std::str::from_utf8(&self.text[start..self.at]).map_err(|e| e.to_string())?;
        if self.eat(b'(') {
            let x = self.sum(depth)?;
            if !self.eat(b')') {
                return Err(format!("`{name}(` is not closed"));
            }
            return match name {
                "sqrt" if x < 0.0 && self.vars.is_some() => Err("sqrt of a negative number".into()),
                "sqrt" => Ok(x.sqrt()),
                "abs" => Ok(x.abs()),
                "sin" => Ok(x.sin()),
                "cos" => Ok(x.cos()),
                "tan" => Ok(x.tan()),
                "exp" => Ok(x.exp()),
                "ln" if x <= 0.0 && self.vars.is_some() => Err("ln of a number ≤ 0".into()),
                "ln" => Ok(x.ln()),
                _ => Err(format!("unknown function `{}`", crate::text::clip(name, MAX_NAME))),
            };
        }
        if name == "pi" {
            return Ok(std::f64::consts::PI);
        }
        match self.vars {
            Some(vars) => vars.get(name).copied().ok_or_else(|| format!("unknown variable `{}`", crate::text::clip(name, MAX_NAME))),
            None => {
                if !self.seen.iter().any(|n| n == name) {
                    self.seen.push(name.to_owned());
                }
                Ok(1.0)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> Variables {
        [("w".to_owned(), 22.86), ("l".to_owned(), 20.0), ("er".to_owned(), 4.0)].into_iter().collect()
    }

    #[test]
    fn arithmetic_with_variables() {
        let v = vars();
        assert_eq!(eval("w", &v), Ok(22.86));
        assert_eq!(eval(" w/2 + 1 ", &v), Ok(22.86 / 2.0 + 1.0));
        assert_eq!(eval("-l + 2*3", &v), Ok(-14.0));
        assert_eq!(eval("2^3^2", &v), Ok(512.0));
        assert_eq!(eval("(1 + 2) * -(3 - 5)", &v), Ok(6.0));
        assert_eq!(eval("sqrt(er) * pi", &v), Ok(2.0 * std::f64::consts::PI));
        assert_eq!(eval("1.5e-3*1e3", &v), Ok(1.5));
        assert!(eval("2e", &v).is_err());
    }

    #[test]
    fn bad_expressions_are_errors_not_panics() {
        let v = vars();
        for bad in ["", "(", "w +", "1/0", "x", "sqrt(-1)", "ln(0)", "foo(1)", "1 2", "é", "w**2", ")", "1e999", "0^-1"] {
            assert!(eval(bad, &v).is_err(), "{bad} should fail");
        }
        let deep = "(".repeat(100) + "1" + &")".repeat(100);
        assert!(eval(&deep, &v).is_err());
        let minus = "-".repeat(200) + "1";
        assert!(eval(&minus, &v).is_err());
        assert!(eval(&"1+".repeat(200), &v).is_err());
    }

    #[test]
    fn names_are_collected_once() {
        assert_eq!(names("a*b + a/c"), Ok(vec!["a".to_owned(), "b".to_owned(), "c".to_owned()]));
        assert_eq!(names("sqrt(pi)"), Ok(vec![]));
        assert!(valid_name("w_1") && !valid_name("1w") && !valid_name("pi") && !valid_name("a-b") && !valid_name(""));
    }
}
