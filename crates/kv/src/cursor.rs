//! A byte cursor shared by the text parsers: position, line tracking, comments, quoted strings.

use crate::Error;

pub(crate) struct Cursor<'a> {
    src: &'a [u8],
    pos: usize,
    line: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(src: &'a str) -> Self {
        Self { src: src.as_bytes(), pos: 0, line: 1 }
    }

    pub fn err<T>(&self, message: impl Into<String>) -> Result<T, Error> {
        Err(Error { line: self.line, message: message.into() })
    }

    pub fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    pub fn starts_with(&self, s: &str) -> bool {
        self.src[self.pos..].starts_with(s.as_bytes())
    }

    pub fn bump(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.pos += 1;
        if c == b'\n' {
            self.line += 1;
        }
        Some(c)
    }

    pub fn advance(&mut self, n: usize) {
        (0..n).for_each(|_| {
            self.bump();
        });
    }

    pub fn expect(&mut self, s: &str) -> Result<(), Error> {
        if !self.starts_with(s) {
            return self.err(format!("expected `{s}`"));
        }
        self.advance(s.len());
        Ok(())
    }

    /// Skip whitespace, `//` line comments and `/* */` block comments.
    pub fn skip_trivia(&mut self) {
        loop {
            match self.peek() {
                Some(c) if c.is_ascii_whitespace() => {
                    self.bump();
                }
                Some(b'/') if self.starts_with("//") => {
                    while self.peek().is_some_and(|c| c != b'\n') {
                        self.bump();
                    }
                }
                Some(b'/') if self.starts_with("/*") => {
                    self.advance(2);
                    while self.peek().is_some() && !self.starts_with("*/") {
                        self.bump();
                    }
                    self.advance(2);
                }
                _ => return,
            }
        }
    }

    /// A `"..."` string with `\n \t \\ \"` escapes; the cursor is on the opening quote.
    pub fn quoted(&mut self) -> Result<String, Error> {
        self.expect("\"")?;
        let mut out = Vec::new();
        loop {
            match self.bump() {
                None => return self.err("unterminated string"),
                Some(b'"') => break,
                Some(b'\\') => out.push(match self.bump() {
                    Some(b'n') => b'\n',
                    Some(b't') => b'\t',
                    Some(c) => c,
                    None => return self.err("unterminated string"),
                }),
                Some(c) => out.push(c),
            }
        }
        Ok(String::from_utf8_lossy(&out).into_owned())
    }

    /// Text up to (not including) `end`, or `None` if `end` never comes.
    pub fn take_until_str(&mut self, end: &str) -> Option<String> {
        let start = self.pos;
        while !self.starts_with(end) {
            self.bump()?;
        }
        Some(String::from_utf8_lossy(&self.src[start..self.pos]).into_owned())
    }

    /// Bytes up to (not including) the first byte `stop` accepts.
    pub fn take_until(&mut self, stop: impl Fn(u8) -> bool) -> &'a str {
        let start = self.pos;
        while self.peek().is_some_and(|c| !stop(c)) {
            self.bump();
        }
        std::str::from_utf8(&self.src[start..self.pos]).unwrap_or("")
    }
}
