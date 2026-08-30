use std::fmt;

/// Where in the document a decode failed. Segments are pushed as the decoder
/// descends and rendered only when an error is actually produced.
#[derive(Debug, Clone, Copy)]
pub enum Segment {
    Key(&'static str),
    Index(usize),
}

#[derive(Debug, Default)]
pub struct PathStack {
    segments: Vec<Segment>,
}

impl PathStack {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_key(&mut self, key: &'static str) {
        self.segments.push(Segment::Key(key));
    }

    pub fn push_index(&mut self, index: usize) {
        self.segments.push(Segment::Index(index));
    }

    pub fn pop(&mut self) {
        self.segments.pop();
    }

    /// Run `f` with `key` pushed, popping it whatever the outcome.
    pub fn in_key<T>(&mut self, key: &'static str, f: impl FnOnce(&mut Self) -> T) -> T {
        self.push_key(key);
        let out = f(self);
        self.pop();
        out
    }

    /// Run `f` with `index` pushed, popping it whatever the outcome.
    pub fn in_index<T>(&mut self, index: usize, f: impl FnOnce(&mut Self) -> T) -> T {
        self.push_index(index);
        let out = f(self);
        self.pop();
        out
    }

    pub fn render(&self) -> String {
        let mut out = String::from("$");
        for segment in &self.segments {
            match segment {
                Segment::Key(k) => {
                    out.push('.');
                    out.push_str(k);
                }
                Segment::Index(i) => {
                    out.push('[');
                    out.push_str(&i.to_string());
                    out.push(']');
                }
            }
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    MissingKey(&'static str),
    WrongType {
        expected: &'static str,
        found: String,
    },
    Malformed(String),
}

/// A decode failure, carrying the JSON path of the offending value (FR-008).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeError {
    pub path: String,
    pub kind: ErrorKind,
}

impl DecodeError {
    pub fn missing(path: &PathStack, key: &'static str) -> Self {
        Self {
            path: path.render(),
            kind: ErrorKind::MissingKey(key),
        }
    }

    pub fn wrong_type(path: &PathStack, expected: &'static str, found: &serde_json::Value) -> Self {
        Self {
            path: path.render(),
            kind: ErrorKind::WrongType {
                expected,
                found: describe(found),
            },
        }
    }

    pub fn malformed(path: &PathStack, message: impl Into<String>) -> Self {
        Self {
            path: path.render(),
            kind: ErrorKind::Malformed(message.into()),
        }
    }
}

pub fn describe(value: &serde_json::Value) -> String {
    use serde_json::Value::*;
    match value {
        Null => "null".into(),
        Bool(_) => "boolean".into(),
        Number(_) => "number".into(),
        String(_) => "string".into(),
        Array(_) => "array".into(),
        Object(_) => "object".into(),
    }
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ErrorKind::MissingKey(k) => write!(f, "{}: missing required key `{}`", self.path, k),
            ErrorKind::WrongType { expected, found } => {
                write!(f, "{}: expected {}, found {}", self.path, expected, found)
            }
            ErrorKind::Malformed(m) => write!(f, "{}: {}", self.path, m),
        }
    }
}

impl std::error::Error for DecodeError {}
