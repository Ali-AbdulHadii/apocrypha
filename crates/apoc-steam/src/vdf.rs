//! A minimal, defensive parser for Valve's KeyValues (VDF) text format.
//!
//! Steam's `libraryfolders.vdf`, `appmanifest_*.acf`, and `config.vdf` all use it:
//!
//! ```text
//! "libraryfolders"
//! {
//!     "0"
//!     {
//!         "path"    "/home/user/.local/share/Steam"
//!         "apps"  { "2246340"  "12345" }
//!     }
//! }
//! ```
//!
//! VDF files are user-writable and therefore untrusted input: the parser is
//! non-recursive in its failure behavior (depth-limited), never panics, and
//! ignores malformed trailing content rather than erroring the whole scan.

use std::collections::BTreeMap;

const MAX_DEPTH: usize = 64;

/// A parsed KeyValues node: either a leaf string or a nested map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Str(String),
    Map(BTreeMap<String, Value>),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn as_map(&self) -> Option<&BTreeMap<String, Value>> {
        match self {
            Value::Map(m) => Some(m),
            _ => None,
        }
    }

    /// Case-insensitive child lookup (Steam is inconsistent about casing).
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_map()?
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
    }

    /// Walk a path of keys, e.g. `v.path(&["Software", "Valve", "Steam"])`.
    pub fn path(&self, keys: &[&str]) -> Option<&Value> {
        let mut cur = self;
        for k in keys {
            cur = cur.get(k)?;
        }
        Some(cur)
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Parser {
            bytes: input.as_bytes(),
            pos: 0,
        }
    }

    fn skip_trivia(&mut self) {
        loop {
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
                self.pos += 1;
            }
            // Line comments: `//`
            if self.pos + 1 < self.bytes.len()
                && self.bytes[self.pos] == b'/'
                && self.bytes[self.pos + 1] == b'/'
            {
                while self.pos < self.bytes.len() && self.bytes[self.pos] != b'\n' {
                    self.pos += 1;
                }
                continue;
            }
            break;
        }
    }

    /// Read a quoted or bare token.
    fn token(&mut self) -> Option<String> {
        self.skip_trivia();
        if self.pos >= self.bytes.len() {
            return None;
        }
        if self.bytes[self.pos] == b'"' {
            self.pos += 1;
            let mut out = String::new();
            while self.pos < self.bytes.len() {
                match self.bytes[self.pos] {
                    b'"' => {
                        self.pos += 1;
                        return Some(out);
                    }
                    b'\\' if self.pos + 1 < self.bytes.len() => {
                        let esc = self.bytes[self.pos + 1];
                        out.push(match esc {
                            b'n' => '\n',
                            b't' => '\t',
                            other => other as char,
                        });
                        self.pos += 2;
                    }
                    c => {
                        out.push(c as char);
                        self.pos += 1;
                    }
                }
            }
            return Some(out);
        }
        if self.bytes[self.pos] == b'{' || self.bytes[self.pos] == b'}' {
            return None;
        }
        let start = self.pos;
        while self.pos < self.bytes.len()
            && !self.bytes[self.pos].is_ascii_whitespace()
            && self.bytes[self.pos] != b'{'
            && self.bytes[self.pos] != b'}'
        {
            self.pos += 1;
        }
        if start == self.pos {
            return None;
        }
        Some(String::from_utf8_lossy(&self.bytes[start..self.pos]).into_owned())
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_trivia();
        self.bytes.get(self.pos).copied()
    }

    fn parse_map(&mut self, depth: usize) -> BTreeMap<String, Value> {
        let mut map = BTreeMap::new();
        if depth > MAX_DEPTH {
            return map;
        }
        loop {
            match self.peek() {
                None => break,
                Some(b'}') => {
                    self.pos += 1;
                    break;
                }
                Some(b'{') => {
                    // Value without a key; skip the block defensively.
                    self.pos += 1;
                    let _ = self.parse_map(depth + 1);
                }
                _ => {
                    let Some(key) = self.token() else {
                        // Unparseable byte; advance to avoid an infinite loop.
                        self.pos += 1;
                        continue;
                    };
                    match self.peek() {
                        Some(b'{') => {
                            self.pos += 1;
                            map.insert(key, Value::Map(self.parse_map(depth + 1)));
                        }
                        None => {
                            map.insert(key, Value::Str(String::new()));
                            break;
                        }
                        _ => {
                            let val = self.token().unwrap_or_default();
                            map.insert(key, Value::Str(val));
                        }
                    }
                }
            }
        }
        map
    }
}

/// Parse a VDF document into its top-level map. Malformed input yields a partial
/// (possibly empty) map rather than an error: callers treat missing keys as
/// "not found", which is the correct behavior for optional Steam files.
pub fn parse(input: &str) -> Value {
    let mut p = Parser::new(input);
    Value::Map(p.parse_map(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_libraryfolders_shape() {
        let src = r#"
"libraryfolders"
{
    "0"
    {
        "path"        "/home/aria/.local/share/Steam"
        "label"       ""
        "apps"
        {
            "2246340"     "70000000"
            "570"         "100"
        }
    }
    "1"
    {
        "path"        "/mnt/games/SteamLibrary"
        "apps"        {  "220"  "5"  }
    }
}
"#;
        let v = parse(src);
        let lf = v.get("libraryfolders").expect("root key");
        let zero = lf.get("0").unwrap();
        assert_eq!(
            zero.get("path").and_then(Value::as_str),
            Some("/home/aria/.local/share/Steam")
        );
        assert!(zero.path(&["apps", "2246340"]).is_some());
        let one = lf.get("1").unwrap();
        assert_eq!(
            one.get("path").and_then(Value::as_str),
            Some("/mnt/games/SteamLibrary")
        );
        assert!(one.path(&["apps", "2246340"]).is_none());
    }

    #[test]
    fn parses_appmanifest_shape() {
        let src = r#"
"AppState"
{
    "appid"       "2246340"
    "name"        "Monster Hunter Wilds"
    "installdir"  "MonsterHunterWilds"
}
"#;
        let v = parse(src);
        assert_eq!(
            v.path(&["AppState", "installdir"]).and_then(Value::as_str),
            Some("MonsterHunterWilds")
        );
    }

    #[test]
    fn case_insensitive_lookup() {
        let v = parse(r#""AppState" { "InstallDir" "X" }"#);
        assert_eq!(
            v.path(&["appstate", "installdir"]).and_then(Value::as_str),
            Some("X")
        );
    }

    #[test]
    fn malformed_input_does_not_panic() {
        for bad in [r#""a" {"#, r#"}}}"#, "", r#""k""#, r#""a" { "b" { "c" "#] {
            let _ = parse(bad);
        }
    }

    #[test]
    fn handles_comments_and_bare_tokens() {
        let v = parse("// leading comment\n\"root\" { key value }");
        assert_eq!(v.path(&["root", "key"]).and_then(Value::as_str), Some("value"));
    }
}
