//! The `apocrypha://` link, parsed as a wire format rather than as a URL.
//!
//! Anyone who can publish a web page can make this process start with an
//! argument of their choosing, so this is the one input in the application that
//! is chosen entirely by a stranger. Everything here exists to make that
//! boring.
//!
//! Two rules shape it.
//!
//! **A link names things; it never supplies them.** The three values it carries
//! are identifiers the service already knows. It cannot say where to download
//! from, where to write, what hash to trust, or what to install — those are
//! re-resolved against the service over a compiled-in origin, and a link that
//! could name any of them would be an instruction rather than a reference.
//!
//! **The grammar is exact, and matched against the raw bytes.** No
//! percent-decoding, no normalisation, no query map, no URL library. Accepting
//! a parsed URL and then enumerating the dangerous shapes is the approach that
//! keeps losing: `%00`, an encoded `&`, a full-width Unicode letter that looks
//! like ASCII, `;` as a separator, a duplicate key that a map silently collapses
//! — each is a separate thing to remember. Refusing every byte that is not in
//! the grammar makes all of them fail at once, without anyone having thought of
//! them first.

/// What a link asked for: three identifiers, and nothing else expressible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallRequest {
    pub game_slug: String,
    pub mod_slug: String,
    pub file_id: String,
}

/// Why a link was refused.
///
/// Deliberately coarse and carrying none of the offending text. A rejected link
/// is attacker-authored, and putting it in a message — which ends up in a log,
/// a terminal, or an error toast — hands them somewhere to put escape sequences
/// and unbounded junk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkError {
    /// Not an `apocrypha://` link at all. Ordinary: other schemes reach the
    /// same entry point.
    NotOurs,
    /// Ours, but not something this version understands.
    Malformed,
}

impl LinkError {
    /// What to show someone. One sentence, and never the link.
    pub fn message(self) -> &'static str {
        match self {
            LinkError::NotOurs => "That is not an Apocrypha link.",
            LinkError::Malformed => {
                "That Apocrypha link is not one this app understands. \
                 It may have been altered, or made for a newer version."
            }
        }
    }
}

/// The scheme and the only action there is.
const PREFIX: &str = "apocrypha://install?";

/// Longest link worth reading.
///
/// Three identifiers of known maximum size cannot approach this. The bound is
/// here because the input arrives as a process argument that something else
/// chose the length of, and everything downstream — copying, comparing,
/// reporting — is cheaper when it cannot be a megabyte.
const MAX_LEN: usize = 256;

/// The longest a slug may be, matching the service's own column.
const MAX_SLUG: usize = 64;

/// Parse a link, or say why not.
///
/// The argument must be exactly a link. An argument that merely *contains* one
/// is refused: the process may be started with any arguments at all, and
/// searching them for something link-shaped is how a value meant as one thing
/// gets read as another.
pub fn parse(raw: &str) -> Result<InstallRequest, LinkError> {
    if !raw.starts_with(PREFIX) {
        // Said apart from the rest so the interface can stay quiet about links
        // meant for someone else, while still explaining a broken one of ours.
        return if raw.starts_with("apocrypha:") {
            Err(LinkError::Malformed)
        } else {
            Err(LinkError::NotOurs)
        };
    }

    if raw.len() > MAX_LEN {
        return Err(LinkError::Malformed);
    }

    // Every byte, before anything is split. `%` and `+` are refused outright
    // rather than decoded: with no decoding step there is no second reading of
    // the string, so no encoded separator, no embedded NUL, and no pair of
    // spellings for one value. Anything non-ASCII goes with them, which is what
    // makes a Cyrillic letter that draws like `a` a parse failure instead of a
    // different mod.
    let query = &raw[PREFIX.len()..];
    if !query.bytes().all(is_allowed_byte) {
        return Err(LinkError::Malformed);
    }

    let mut game: Option<&str> = None;
    let mut module: Option<&str> = None;
    let mut file: Option<&str> = None;
    let mut seen = 0usize;

    for pair in query.split('&') {
        seen += 1;
        if seen > 3 {
            return Err(LinkError::Malformed);
        }

        // `split_once`, so a value containing `=` is refused rather than being
        // quietly truncated at the first one.
        let (key, value) = match pair.split_once('=') {
            Some((k, v)) if !k.is_empty() && !v.is_empty() && !v.contains('=') => (k, v),
            _ => return Err(LinkError::Malformed),
        };

        // Each key exactly once. Checked here rather than by collecting into a
        // map, because a map answers "what is the value of game" by silently
        // preferring one of the two that were sent — and which one it prefers
        // is not something a reader of this code would know.
        let slot = match key {
            "game" => &mut game,
            "mod" => &mut module,
            "file" => &mut file,
            _ => return Err(LinkError::Malformed),
        };
        if slot.is_some() {
            return Err(LinkError::Malformed);
        }
        *slot = Some(value);
    }

    let (Some(game), Some(module), Some(file)) = (game, module, file) else {
        return Err(LinkError::Malformed);
    };

    if !is_slug(game) || !is_slug(module) || !is_uuid(file) {
        return Err(LinkError::Malformed);
    }

    Ok(InstallRequest {
        game_slug: game.to_string(),
        mod_slug: module.to_string(),
        file_id: file.to_string(),
    })
}

/// The only bytes a link may contain after the prefix.
///
/// Lowercase letters, digits, and the four punctuation marks the grammar needs.
/// Note what is absent: `%`, `+`, `#`, `?`, `;`, `/`, `@`, `:`, whitespace,
/// every control character, and every byte above 0x7F.
fn is_allowed_byte(b: u8) -> bool {
    b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'&' | b'=' | b'_')
}

/// A service slug: lowercase, and never starting or ending with a hyphen.
fn is_slug(s: &str) -> bool {
    if s.is_empty() || s.len() > MAX_SLUG {
        return false;
    }
    if s.starts_with('-') || s.ends_with('-') {
        return false;
    }
    s.bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A UUID in exactly one spelling: lowercase hex, hyphenated, 8-4-4-4-12.
///
/// The aliases are refused deliberately — braces, a `urn:uuid:` prefix, no
/// hyphens, uppercase hex. They all name the same thing, and accepting several
/// spellings of one value means anything comparing them has to agree on which
/// spelling it holds.
fn is_uuid(s: &str) -> bool {
    const GROUPS: [usize; 5] = [8, 4, 4, 4, 12];
    let mut parts = s.split('-');
    for len in GROUPS {
        match parts.next() {
            Some(p)
                if p.len() == len
                    && p.bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) => {}
            _ => return false,
        }
    }
    parts.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str =
        "apocrypha://install?game=monster-hunter-wilds&mod=reframework&file=3f2504e0-4f89-41d3-9a0c-0305e82c3301";

    fn parsed(raw: &str) -> InstallRequest {
        parse(raw).expect("should parse")
    }

    #[test]
    fn an_ordinary_link_parses() {
        let r = parsed(GOOD);
        assert_eq!(r.game_slug, "monster-hunter-wilds");
        assert_eq!(r.mod_slug, "reframework");
        assert_eq!(r.file_id, "3f2504e0-4f89-41d3-9a0c-0305e82c3301");
    }

    #[test]
    fn the_order_of_the_three_does_not_matter() {
        let r = parsed(
            "apocrypha://install?file=3f2504e0-4f89-41d3-9a0c-0305e82c3301&game=cyberpunk-2077&mod=a",
        );
        assert_eq!(r.game_slug, "cyberpunk-2077");
        assert_eq!(r.mod_slug, "a");
    }

    #[test]
    fn another_scheme_is_not_ours_rather_than_broken() {
        // nxm:// reaches the same entry point, and calling someone else's link
        // malformed would put an error on screen for a working feature.
        assert_eq!(
            parse("nxm://www.nexusmods.com/x/mods/1"),
            Err(LinkError::NotOurs)
        );
        assert_eq!(parse("https://apocryphamods.com/"), Err(LinkError::NotOurs));
        assert_eq!(parse(""), Err(LinkError::NotOurs));
    }

    #[test]
    fn nothing_percent_encoded_is_accepted() {
        // The whole point of refusing `%`: with no decoding step there is no
        // second reading of the string, so none of these can become a
        // separator, a NUL, or a path.
        for bad in [
            "apocrypha://install?game=a%00b&mod=m&file=3f2504e0-4f89-41d3-9a0c-0305e82c3301",
            "apocrypha://install?game=a%26mod=evil&mod=m&file=3f2504e0-4f89-41d3-9a0c-0305e82c3301",
            "apocrypha://install?game=%2e%2e%2f&mod=m&file=3f2504e0-4f89-41d3-9a0c-0305e82c3301",
            "apocrypha://install?game=a+b&mod=m&file=3f2504e0-4f89-41d3-9a0c-0305e82c3301",
        ] {
            assert_eq!(parse(bad), Err(LinkError::Malformed), "accepted {bad}");
        }
    }

    #[test]
    fn a_letter_that_only_looks_like_ascii_is_refused() {
        // Cyrillic small a. Renders like `a`, is not `a`, and would name a
        // different mod than the one a person read off the screen.
        let bad =
            "apocrypha://install?game=\u{0430}&mod=m&file=3f2504e0-4f89-41d3-9a0c-0305e82c3301";
        assert_eq!(parse(bad), Err(LinkError::Malformed));
    }

    #[test]
    fn a_repeated_key_is_refused_rather_than_resolved() {
        // The failure a query map hides: two answers, one silently chosen, and
        // no way to tell from the call site which.
        let bad =
            "apocrypha://install?game=a&game=b&mod=m&file=3f2504e0-4f89-41d3-9a0c-0305e82c3301";
        assert_eq!(parse(bad), Err(LinkError::Malformed));
    }

    #[test]
    fn extra_or_missing_parameters_are_refused() {
        for bad in [
            // A fourth pair, even a harmless-looking one.
            "apocrypha://install?game=a&mod=m&file=3f2504e0-4f89-41d3-9a0c-0305e82c3301&url=x",
            // Two of the three.
            "apocrypha://install?game=a&mod=m",
            "apocrypha://install?",
            // Trailing separator, empty pair, no value.
            "apocrypha://install?game=a&mod=m&file=3f2504e0-4f89-41d3-9a0c-0305e82c3301&",
            "apocrypha://install?game=a&&mod=m&file=3f2504e0-4f89-41d3-9a0c-0305e82c3301",
            "apocrypha://install?game=&mod=m&file=3f2504e0-4f89-41d3-9a0c-0305e82c3301",
        ] {
            assert_eq!(parse(bad), Err(LinkError::Malformed), "accepted {bad}");
        }
    }

    #[test]
    fn only_the_install_action_exists() {
        for bad in [
            "apocrypha://run?game=a&mod=m&file=3f2504e0-4f89-41d3-9a0c-0305e82c3301",
            "apocrypha://install/../x?game=a&mod=m&file=3f2504e0-4f89-41d3-9a0c-0305e82c3301",
            "apocrypha://INSTALL?game=a&mod=m&file=3f2504e0-4f89-41d3-9a0c-0305e82c3301",
            "apocrypha://user@install?game=a&mod=m&file=3f2504e0-4f89-41d3-9a0c-0305e82c3301",
        ] {
            assert_eq!(parse(bad), Err(LinkError::Malformed), "accepted {bad}");
        }
    }

    #[test]
    fn a_uuid_has_exactly_one_spelling() {
        let with = |f: &str| format!("apocrypha://install?game=a&mod=m&file={f}");
        for bad in [
            "3F2504E0-4F89-41D3-9A0C-0305E82C3301",   // uppercase
            "3f2504e04f8941d39a0c0305e82c3301",       // unhyphenated
            "{3f2504e0-4f89-41d3-9a0c-0305e82c3301}", // braced
            "3f2504e0-4f89-41d3-9a0c-0305e82c330",    // short
            "3f2504e0-4f89-41d3-9a0c-0305e82c33011",  // long
            "3f2504e0-4f89-41d3-9a0c-0305e82c3301-",  // trailing group
            "zf2504e0-4f89-41d3-9a0c-0305e82c3301",   // not hex
        ] {
            assert_eq!(
                parse(&with(bad)),
                Err(LinkError::Malformed),
                "accepted {bad}"
            );
        }
        assert!(parse(&with("3f2504e0-4f89-41d3-9a0c-0305e82c3301")).is_ok());
    }

    #[test]
    fn a_slug_cannot_be_a_path_or_a_hostname() {
        let with = |g: &str| {
            format!("apocrypha://install?game={g}&mod=m&file=3f2504e0-4f89-41d3-9a0c-0305e82c3301")
        };
        for bad in [
            "../../etc",
            "a/b",
            "a.b",
            "a:b",
            "-lead",
            "trail-",
            "UPPER",
            "a b",
        ] {
            assert_eq!(
                parse(&with(bad)),
                Err(LinkError::Malformed),
                "accepted {bad}"
            );
        }
        assert!(
            parse(&with("a")).is_ok(),
            "a one-character slug is legitimate"
        );
    }

    #[test]
    fn an_absurdly_long_link_is_refused_before_it_is_split() {
        let long = format!(
            "apocrypha://install?game={}&mod=m&file=3f2504e0-4f89-41d3-9a0c-0305e82c3301",
            "a".repeat(1024)
        );
        assert_eq!(parse(&long), Err(LinkError::Malformed));
    }

    #[test]
    fn a_slug_longer_than_the_service_allows_is_refused() {
        let with = |g: String| {
            format!("apocrypha://install?game={g}&mod=m&file=3f2504e0-4f89-41d3-9a0c-0305e82c3301")
        };
        assert!(parse(&with("a".repeat(MAX_SLUG))).is_ok());
        assert_eq!(
            parse(&with("a".repeat(MAX_SLUG + 1))),
            Err(LinkError::Malformed)
        );
    }

    #[test]
    fn an_argument_that_merely_contains_a_link_is_not_one() {
        // The process can be started with anything. Searching arguments for
        // something link-shaped is how a value meant as one thing is read as
        // another, so the argument has to *be* the link.
        for bad in [
            &format!("--config={GOOD}"),
            &format!(" {GOOD}"),
            &format!("{GOOD} --install-to=/etc"),
            &format!("x{GOOD}"),
        ] {
            assert!(parse(bad).is_err(), "accepted {bad}");
        }
        assert!(parse(GOOD).is_ok());
    }

    #[test]
    fn a_refusal_never_carries_the_link() {
        // A rejected link is written by whoever wants it rejected, and a message
        // built from it is somewhere to put escape sequences and unbounded junk.
        let nasty = "apocrypha://install?game=\u{1b}[2Jwiped&mod=m&file=x";
        let err = parse(nasty).expect_err("refused");
        assert!(!err.message().contains("wiped"));
        assert!(!err.message().contains('\u{1b}'));
    }
}
