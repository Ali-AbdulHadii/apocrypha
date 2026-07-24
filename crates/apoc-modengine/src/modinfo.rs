//! Parser for Fluffy Mod Manager `modinfo.ini` files.
//!
//! Format: simple `key=value` lines (CRLF), with a literal backslash-n (`\n`) in
//! the `description` value used as a line break. Keys seen in the wild:
//! `name`, `version`, `description`, `category`, `screenshot`, `author`,
//! `NameAsBundle`. Lookups are case-insensitive; original text is preserved.

use std::collections::BTreeMap;

/// A parsed `modinfo.ini`. Keys are stored with their original casing; use the
/// accessors for case-insensitive lookup.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Modinfo {
    pub raw: BTreeMap<String, String>,
}

impl Modinfo {
    pub fn parse(text: &str) -> Self {
        let mut raw = BTreeMap::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(['#', ';']) {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                raw.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
        Modinfo { raw }
    }

    /// Case-insensitive lookup. An empty value (`screenshot=` with nothing after
    /// it, as the sample's "segment" header folders use) means *not set*, so it
    /// reads as `None` rather than an empty string.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.raw
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
            .filter(|v| !v.is_empty())
    }

    pub fn name(&self) -> Option<&str> {
        self.get("name")
    }

    pub fn version(&self) -> Option<&str> {
        self.get("version")
    }

    pub fn category(&self) -> Option<&str> {
        self.get("category")
    }

    pub fn author(&self) -> Option<&str> {
        self.get("author")
    }

    pub fn screenshot(&self) -> Option<&str> {
        self.get("screenshot")
    }

    /// The grouping key that collapses many options into one logical bundle.
    pub fn name_as_bundle(&self) -> Option<&str> {
        self.get("NameAsBundle")
    }

    /// Description with the literal `\n` / `\r\n` escapes turned into real breaks.
    pub fn description(&self) -> Option<String> {
        self.get("description").map(unescape_description)
    }
}

/// Turn Fluffy's literal escape sequences into real line breaks.
pub fn unescape_description(s: &str) -> String {
    s.replace("\\r\\n", "\n").replace("\\n", "\n").replace("\\r", "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sample_basic_option() {
        let ini = "name=-0- basic files (must install)\r\n\
                   version=v1.03\r\n\
                   description=!!! must install this...\\nVer.R Hirabami F-M for WSR body...\r\n\
                   category=Armor\r\n\
                   screenshot=basic.jpg\r\n\
                   author=Ranaragua\r\n\
                   NameAsBundle=Ver.R Hirabami F-M Armor\r\n";
        let m = Modinfo::parse(ini);
        assert_eq!(m.name(), Some("-0- basic files (must install)"));
        assert_eq!(m.version(), Some("v1.03"));
        assert_eq!(m.category(), Some("Armor"));
        assert_eq!(m.author(), Some("Ranaragua"));
        assert_eq!(m.screenshot(), Some("basic.jpg"));
        assert_eq!(m.name_as_bundle(), Some("Ver.R Hirabami F-M Armor"));
        let desc = m.description().unwrap();
        assert!(desc.contains('\n'), "literal \\n must become a real newline");
        assert!(desc.starts_with("!!! must install this..."));
    }

    #[test]
    fn empty_values_read_as_unset() {
        // The sample's "segment" header folders write `screenshot=` with no value.
        let m = Modinfo::parse("name=-1- Helm\nscreenshot=\ncategory=Armor\n");
        assert_eq!(m.screenshot(), None, "empty value must not become Some(\"\")");
        assert_eq!(m.name(), Some("-1- Helm"));
        assert_eq!(m.category(), Some("Armor"));
    }

    #[test]
    fn case_insensitive_keys() {
        let m = Modinfo::parse("NAMEasBundle=X\nName=Y\n");
        assert_eq!(m.name_as_bundle(), Some("X"));
        assert_eq!(m.name(), Some("Y"));
    }
}
