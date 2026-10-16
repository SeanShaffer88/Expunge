//! JSON without a library: a strict RFC 8259 reader and a writer whose
//! compact and pretty layouts match what earlier versions of Expunge
//! produced, so every saved log, setting, backup and package stays
//! readable and diffs cleanly.
//!
//! Objects keep their keys in insertion order (the order fields are
//! written), and reading rejects duplicate keys, invalid escapes, lone
//! surrogates, trailing garbage and nesting deeper than [`MAX_DEPTH`].

use std::fmt::Write as _;

/// Deepest nesting accepted when reading (guards the stack).
pub const MAX_DEPTH: usize = 256;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    /// Integers that fit keep their exact value.
    Int(i64),
    /// Integers too large for `i64`.
    UInt(u64),
    Float(f64),
    String(String),
    Array(Vec<Value>),
    /// Keys in order.
    Object(Vec<(String, Value)>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            Value::Int(i) => Some(i),
            Value::UInt(u) => i64::try_from(u).ok(),
            Value::Float(f) if f.fract() == 0.0 && f.abs() < 9.0e15 => Some(f as i64),
            _ => None,
        }
    }
    pub fn as_u64(&self) -> Option<u64> {
        match *self {
            Value::Int(i) => u64::try_from(i).ok(),
            Value::UInt(u) => Some(u),
            Value::Float(f) if f.fract() == 0.0 && (0.0..9.0e15).contains(&f) => Some(f as u64),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match *self {
            Value::Int(i) => Some(i as f64),
            Value::UInt(u) => Some(u as f64),
            Value::Float(f) => Some(f),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match *self {
            Value::Bool(b) => Some(b),
            _ => None,
        }
    }
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }
    pub fn as_object(&self) -> Option<&[(String, Value)]> {
        match self {
            Value::Object(m) => Some(m),
            _ => None,
        }
    }
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
}

impl std::ops::Index<&str> for Value {
    type Output = Value;
    /// `v["key"]`: [`Value::Null`] when absent, as in JavaScript.
    fn index(&self, key: &str) -> &Value {
        static NULL: Value = Value::Null;
        self.get(key).unwrap_or(&NULL)
    }
}

impl std::ops::Index<usize> for Value {
    type Output = Value;
    fn index(&self, i: usize) -> &Value {
        static NULL: Value = Value::Null;
        match self {
            Value::Array(a) => a.get(i).unwrap_or(&NULL),
            _ => &NULL,
        }
    }
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub msg: String,
    /// Byte offset in the input.
    pub at: usize,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at byte {}", self.msg, self.at)
    }
}

impl std::error::Error for Error {}

struct Parser<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Parser<'a> {
    fn err<T>(&self, msg: &str) -> Result<T, Error> {
        Err(Error {
            msg: msg.to_string(),
            at: self.at,
        })
    }

    fn ws(&mut self) {
        while let Some(&c) = self.b.get(self.at) {
            if matches!(c, b' ' | b'\t' | b'\n' | b'\r') {
                self.at += 1;
            } else {
                break;
            }
        }
    }

    fn eat(&mut self, lit: &[u8]) -> bool {
        if self.b[self.at..].starts_with(lit) {
            self.at += lit.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, Error> {
        if depth > MAX_DEPTH {
            return self.err("nested too deeply");
        }
        self.ws();
        match self.b.get(self.at) {
            None => self.err("unexpected end"),
            Some(b'n') if self.eat(b"null") => Ok(Value::Null),
            Some(b't') if self.eat(b"true") => Ok(Value::Bool(true)),
            Some(b'f') if self.eat(b"false") => Ok(Value::Bool(false)),
            Some(b'"') => Ok(Value::String(self.string()?)),
            Some(b'[') => {
                self.at += 1;
                let mut a = Vec::new();
                self.ws();
                if self.eat(b"]") {
                    return Ok(Value::Array(a));
                }
                loop {
                    a.push(self.value(depth + 1)?);
                    self.ws();
                    if self.eat(b",") {
                        continue;
                    }
                    if self.eat(b"]") {
                        return Ok(Value::Array(a));
                    }
                    return self.err("expected , or ]");
                }
            }
            Some(b'{') => {
                self.at += 1;
                let mut m: Vec<(String, Value)> = Vec::new();
                self.ws();
                if self.eat(b"}") {
                    return Ok(Value::Object(m));
                }
                loop {
                    self.ws();
                    if self.b.get(self.at) != Some(&b'"') {
                        return self.err("expected a key");
                    }
                    let key_at = self.at;
                    let k = self.string()?;
                    if m.iter().any(|(x, _)| *x == k) {
                        return Err(Error {
                            msg: format!("duplicate key {k:?}"),
                            at: key_at,
                        });
                    }
                    self.ws();
                    if !self.eat(b":") {
                        return self.err("expected :");
                    }
                    let v = self.value(depth + 1)?;
                    m.push((k, v));
                    self.ws();
                    if self.eat(b",") {
                        continue;
                    }
                    if self.eat(b"}") {
                        return Ok(Value::Object(m));
                    }
                    return self.err("expected , or }");
                }
            }
            Some(c) if *c == b'-' || c.is_ascii_digit() => self.number(),
            Some(_) => self.err("unexpected character"),
        }
    }

    fn number(&mut self) -> Result<Value, Error> {
        let start = self.at;
        let digits = |p: &mut Self| {
            let s = p.at;
            while p.b.get(p.at).is_some_and(u8::is_ascii_digit) {
                p.at += 1;
            }
            p.at - s
        };
        self.eat(b"-");
        let int_start = self.at;
        let n = digits(self);
        if n == 0 || (n > 1 && self.b[int_start] == b'0') {
            return self.err("bad number");
        }
        let mut float = false;
        if self.eat(b".") {
            float = true;
            if digits(self) == 0 {
                return self.err("bad number");
            }
        }
        if matches!(self.b.get(self.at), Some(b'e' | b'E')) {
            float = true;
            self.at += 1;
            if matches!(self.b.get(self.at), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if digits(self) == 0 {
                return self.err("bad number");
            }
        }
        let text = std::str::from_utf8(&self.b[start..self.at]).unwrap();
        if !float {
            if let Ok(i) = text.parse::<i64>() {
                return Ok(Value::Int(i));
            }
            if let Ok(u) = text.parse::<u64>() {
                return Ok(Value::UInt(u));
            }
        }
        match text.parse::<f64>() {
            Ok(f) if f.is_finite() => Ok(Value::Float(f)),
            _ => self.err("number out of range"),
        }
    }

    fn hex4(&mut self) -> Result<u32, Error> {
        let Some(h) = self.b.get(self.at..self.at + 4) else {
            return self.err("bad \\u escape");
        };
        let s = std::str::from_utf8(h).map_err(|_| Error {
            msg: "bad \\u escape".into(),
            at: self.at,
        })?;
        let v = u32::from_str_radix(s, 16).map_err(|_| Error {
            msg: "bad \\u escape".into(),
            at: self.at,
        })?;
        self.at += 4;
        Ok(v)
    }

    fn string(&mut self) -> Result<String, Error> {
        self.at += 1; // opening quote
        let mut out = String::new();
        loop {
            // Copy the run up to the next quote, backslash or control char.
            let run_start = self.at;
            while let Some(&c) = self.b.get(self.at) {
                if c == b'"' || c == b'\\' || c < 0x20 {
                    break;
                }
                self.at += 1;
            }
            match std::str::from_utf8(&self.b[run_start..self.at]) {
                Ok(s) => out.push_str(s),
                Err(_) => return self.err("invalid UTF-8"),
            }
            match self.b.get(self.at) {
                None => return self.err("unterminated string"),
                Some(b'"') => {
                    self.at += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.at += 1;
                    let Some(&e) = self.b.get(self.at) else {
                        return self.err("unterminated string");
                    };
                    self.at += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hi = self.hex4()?;
                            let c = if (0xD800..0xDC00).contains(&hi) {
                                if !self.eat(b"\\u") {
                                    return self.err("lone surrogate");
                                }
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return self.err("lone surrogate");
                                }
                                0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                            } else if (0xDC00..0xE000).contains(&hi) {
                                return self.err("lone surrogate");
                            } else {
                                hi
                            };
                            out.push(char::from_u32(c).unwrap());
                        }
                        _ => return self.err("bad escape"),
                    }
                }
                Some(_) => return self.err("control character in string"),
            }
        }
    }
}

/// Read a JSON document (one value, surrounded only by whitespace).
pub fn parse(text: &str) -> Result<Value, Error> {
    parse_bytes(text.as_bytes())
}

pub fn parse_bytes(b: &[u8]) -> Result<Value, Error> {
    let b = b.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(b);
    let mut p = Parser { b, at: 0 };
    let v = p.value(0)?;
    p.ws();
    if p.at != b.len() {
        return p.err("unexpected text after the value");
    }
    Ok(v)
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

fn write_str(out: &mut String, s: &str) {
    out.push('"');
    let mut start = 0;
    for (i, c) in s.char_indices() {
        let esc = match c {
            '"' => "\\\"",
            '\\' => "\\\\",
            '\n' => "\\n",
            '\r' => "\\r",
            '\t' => "\\t",
            '\u{8}' => "\\b",
            '\u{c}' => "\\f",
            c if (c as u32) < 0x20 => "",
            _ => continue,
        };
        out.push_str(&s[start..i]);
        if esc.is_empty() {
            let _ = write!(out, "\\u{:04x}", c as u32);
        } else {
            out.push_str(esc);
        }
        start = i + c.len_utf8();
    }
    out.push_str(&s[start..]);
    out.push('"');
}

/// Floats in ryū's layout (what serde_json writes): shortest digits that
/// read back exactly, plain notation for decimal exponents from -5 to 16,
/// scientific beyond (`1e-7`, `1e+16`).
fn write_f64(out: &mut String, f: f64) {
    if !f.is_finite() {
        out.push_str("null");
        return;
    }
    if f == 0.0 {
        out.push_str(if f.is_sign_negative() { "-0.0" } else { "0.0" });
        return;
    }
    // `{:e}` gives the shortest round-trip digits: "-1.2345e-7".
    let sci = format!("{f:e}");
    let (mant, exp) = sci.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let neg = mant.starts_with('-');
    let digits: String = mant.chars().filter(|c| c.is_ascii_digit()).collect();
    let len = digits.len() as i32;
    let kk = exp + 1; // position of the decimal point
    let k = kk - len; // exponent of the last digit
    if neg {
        out.push('-');
    }
    if (0..=16).contains(&kk) && k >= 0 {
        out.push_str(&digits);
        for _ in 0..k {
            out.push('0');
        }
        out.push_str(".0");
    } else if 0 < kk && kk <= 16 {
        out.push_str(&digits[..kk as usize]);
        out.push('.');
        out.push_str(&digits[kk as usize..]);
    } else if -5 < kk && kk <= 0 {
        out.push_str("0.");
        for _ in 0..-kk {
            out.push('0');
        }
        out.push_str(&digits);
    } else {
        let e = kk - 1;
        let sign = if e >= 0 { "+" } else { "" };
        if len == 1 {
            let _ = write!(out, "{digits}e{sign}{e}");
        } else {
            let _ = write!(out, "{}.{}e{sign}{e}", &digits[..1], &digits[1..]);
        }
    }
}

fn write_value(out: &mut String, v: &Value, indent: Option<usize>) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(i) => {
            let _ = write!(out, "{i}");
        }
        Value::UInt(u) => {
            let _ = write!(out, "{u}");
        }
        Value::Float(f) => write_f64(out, *f),
        Value::String(s) => write_str(out, s),
        Value::Array(a) => {
            if a.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                if let Some(n) = indent {
                    newline(out, n + 1);
                }
                write_value(out, x, indent.map(|n| n + 1));
            }
            if let Some(n) = indent {
                newline(out, n);
            }
            out.push(']');
        }
        Value::Object(m) => {
            if m.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (i, (k, x)) in m.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                if let Some(n) = indent {
                    newline(out, n + 1);
                }
                write_str(out, k);
                out.push(':');
                if indent.is_some() {
                    out.push(' ');
                }
                write_value(out, x, indent.map(|n| n + 1));
            }
            if let Some(n) = indent {
                newline(out, n);
            }
            out.push('}');
        }
    }
}

fn newline(out: &mut String, depth: usize) {
    out.push('\n');
    for _ in 0..depth {
        out.push_str("  ");
    }
}

/// One line, no spaces.
pub fn to_string(v: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, v, None);
    out
}

/// Two-space indentation, `"key": value`.
pub fn to_string_pretty(v: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, v, Some(0));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_every_kind_of_value() {
        let v = parse(
            r#" {"a": [1, -2, 3.5, 1e3, 18446744073709551615, true, false, null],
            "s": "q\"\\\/\b\f\n\r\t\u00e9\ud83d\ude00", "o": {}, "e": []} "#,
        )
        .unwrap();
        assert_eq!(v["a"][0], Value::Int(1));
        assert_eq!(v["a"][1], Value::Int(-2));
        assert_eq!(v["a"][2], Value::Float(3.5));
        assert_eq!(v["a"][3], Value::Float(1000.0));
        assert_eq!(v["a"][4], Value::UInt(u64::MAX));
        assert_eq!(v["a"][5], Value::Bool(true));
        assert!(v["a"][7].is_null());
        assert_eq!(v["s"].as_str().unwrap(), "q\"\\/\u{8}\u{c}\n\r\té😀");
        assert_eq!(v["o"], Value::Object(vec![]));
        assert!(v["missing"].is_null() && v["a"][99].is_null());
    }

    #[test]
    fn rejects_what_isnt_json() {
        for bad in [
            "",
            "{",
            "[1,]",
            "{\"a\":1,}",
            "01",
            "1.",
            ".5",
            "-",
            "1e",
            "+1",
            "\"\\x\"",
            "\"\\ud83d\"",
            "\"\\ude00\"",
            "\"a\nb\"",
            "{\"a\":1}{",
            "{a:1}",
            "{\"a\":1,\"a\":2}",
            "nul",
            "[1 2]",
            "\"\u{0}\"",
            "1e999",
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
        let deep = "[".repeat(MAX_DEPTH + 2) + &"]".repeat(MAX_DEPTH + 2);
        assert!(parse(&deep).is_err());
        let ok = "[".repeat(MAX_DEPTH) + &"]".repeat(MAX_DEPTH);
        assert!(parse(&ok).is_ok());
        assert!(parse_bytes(b"\"\xff\"").is_err(), "invalid UTF-8");
        assert!(parse_bytes(b"\xEF\xBB\xBF{}").is_ok(), "byte-order mark");
    }

    #[test]
    fn writes_like_earlier_versions() {
        let v = Value::Object(vec![
            ("name".into(), Value::String("A \"b\"\n\u{1}".into())),
            ("n".into(), Value::Int(-3)),
            ("f".into(), Value::Float(2.0)),
            ("g".into(), Value::Float(0.25)),
            (
                "list".into(),
                Value::Array(vec![Value::Bool(true), Value::Null]),
            ),
            ("empty".into(), Value::Array(vec![])),
            ("obj".into(), Value::Object(vec![])),
        ]);
        assert_eq!(
            to_string(&v),
            r#"{"name":"A \"b\"\n\u0001","n":-3,"f":2.0,"g":0.25,"list":[true,null],"empty":[],"obj":{}}"#
        );
        assert_eq!(
            to_string_pretty(&v),
            "{\n  \"name\": \"A \\\"b\\\"\\n\\u0001\",\n  \"n\": -3,\n  \"f\": 2.0,\n  \"g\": 0.25,\n  \"list\": [\n    true,\n    null\n  ],\n  \"empty\": [],\n  \"obj\": {}\n}"
        );
    }

    #[test]
    fn matches_serde_json_byte_for_byte() {
        // Cross-check against the library this replaces, while it's still here.
// serde_json's Value sorts keys (derived structs keep field order,
        // which ours does too): keep this sample sorted.
        let sample = r#"{"a":[[],{},[1,[2]],{"k":null}],"big":18446744073709551615,"created":"2026-10-16 05:07:09",
            "f":1.5,"id":"x1","n":42,"neg":-7,"s":"tab\there \u00e9 \ud83d\ude00 \u0007","t":true,"tiny":1e-7,"whole":3.0}"#;
        let ours = parse(sample).unwrap();
        let theirs: serde_json::Value = serde_json::from_str(sample).unwrap();
        assert_eq!(to_string(&ours), serde_json::to_string(&theirs).unwrap());
        assert_eq!(
            to_string_pretty(&ours),
            serde_json::to_string_pretty(&theirs).unwrap()
        );
        // And each reads the other's output.
        assert_eq!(
            parse(&serde_json::to_string_pretty(&theirs).unwrap()).unwrap(),
            ours
        );
        for f in [
            0.0,
            -0.0,
            1.0,
            3.0,
            0.5,
            0.1,
            1e-7,
            1.5e-7,
            0.001,
            0.0001,
            0.00001,
            123456.789,
            1e15,
            1e16,
            1e17,
            1.2345e20,
            9007199254740993.0,
            f64::MIN_POSITIVE,
            f64::MAX,
            -2.5e-300,
            std::f64::consts::PI,
        ] {
            let mut ours = String::new();
            write_f64(&mut ours, f);
            assert_eq!(ours, serde_json::to_string(&f).unwrap(), "{f:e}");
            assert_eq!(
                parse(&ours).unwrap().as_f64().unwrap().to_bits(),
                f.to_bits(),
                "{ours}"
            );
        }
    }
}
