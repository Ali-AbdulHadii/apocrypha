//! Parser for the Fluffy AIO folder-naming convention.
//!
//! Option folders look like `Ver.R Hirabami F-M -1- Helm-01-vanilla`, where:
//!   * ` -1- ` is the **group index** token (helm/body/arm/... buckets),
//!   * `Helm` is a group **label** word,
//!   * `01` (or `addon-01`, `segment`, `cover`, `warning`) is the **slot** token.
//!
//! We deliberately extract only what is robust from the name (group index + a
//! best-effort slot/label). The authoritative *role* of an option is decided in
//! `normalize` from the slot token, payload presence, and `modinfo` description: //! never from the folder name alone.

use regex::Regex;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Naming {
    pub group_index: Option<u32>,
    pub slot_token: Option<String>,
    pub label_word: Option<String>,
    /// The remainder of the folder name after the ` -N- ` group token (or the
    /// whole name if there is no group token). Used to cluster radio sets.
    pub body: String,
}

/// Derive the mutually-exclusive radio-set key for an exclusive option, so that
/// e.g. `Physics-body-*` and `Physics-leg-*` form two independent "pick one"
/// sets even though they share one group token. Clustering strips the trailing
/// variant descriptor: `Helm-01-vanilla` -> `1:helm`, `Physics-leg-none` ->
/// `<g>:physics-leg`.
pub fn radio_set_key(group_index: Option<u32>, body: &str) -> String {
    let num_re = Regex::new(r"-\d+").expect("valid regex");
    let base = if let Some(m) = num_re.find(body) {
        // Numeric slot: cluster on everything before it (e.g. "Helm").
        &body[..m.start()]
    } else {
        // No numeric slot: drop the last dash segment (e.g. "Physics-body-heavy"
        // -> "Physics-body"), leaving one radio set per sub-part.
        match body.rfind('-') {
            Some(i) => &body[..i],
            None => body,
        }
    };
    let base = base.trim_matches(|c: char| c == '-' || c.is_whitespace());
    format!(
        "{}:{}",
        group_index.map(|i| i.to_string()).unwrap_or_default(),
        base.to_ascii_lowercase()
    )
}

/// Parse a Fluffy option folder name into its structural tokens.
pub fn parse_folder(folder: &str) -> Naming {
    // ` -<digits>- ` group token, e.g. " -1- ".
    let group_re = Regex::new(r"\s-(\d+)-\s").expect("valid regex");
    // `addon-<digits>` or `addon<digits>` stackable slot.
    let addon_re = Regex::new(r"(?i)addon-?(\d+)").expect("valid regex");
    // `-<digits>-` numeric variant slot inside the body, e.g. "-01-".
    let variant_re = Regex::new(r"-(\d+)-").expect("valid regex");
    // Leading alphabetic label word of the body, e.g. "Helm".
    let label_re = Regex::new(r"^([A-Za-z][A-Za-z ]*?)(?:-|$)").expect("valid regex");

    let mut naming = Naming::default();

    let body = if let Some(caps) = group_re.captures(folder) {
        naming.group_index = caps.get(1).and_then(|m| m.as_str().parse().ok());
        &folder[caps.get(0).unwrap().end()..]
    } else {
        folder
    };

    // Slot: prefer addon (stackable), then a numeric variant, then keywords.
    if let Some(caps) = addon_re.captures(body) {
        naming.slot_token = Some(format!("addon-{}", &caps[1]));
    } else if let Some(caps) = variant_re.captures(body) {
        naming.slot_token = Some(caps[1].to_string());
    } else {
        let lower = body.to_ascii_lowercase();
        for kw in ["segment", "cover", "warning"] {
            if lower.contains(kw) {
                naming.slot_token = Some(kw.to_string());
                break;
            }
        }
    }

    if let Some(caps) = label_re.captures(body) {
        let word = caps[1].trim();
        if !word.is_empty() {
            naming.label_word = Some(word.to_string());
        }
    }

    naming.body = body.trim().to_string();
    naming
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(s: &str) -> Naming {
        parse_folder(s)
    }

    #[test]
    fn exclusive_variant() {
        let r = n("Ver.R Hirabami F-M -1- Helm-01-vanilla");
        assert_eq!(r.group_index, Some(1));
        assert_eq!(r.slot_token.as_deref(), Some("01"));
        assert_eq!(r.label_word.as_deref(), Some("Helm"));
    }

    #[test]
    fn addon_slot() {
        let r = n("Ver.R Hirabami F-M -2- Body-addon-03-vest only");
        assert_eq!(r.group_index, Some(2));
        assert_eq!(r.slot_token.as_deref(), Some("addon-03"));
        assert_eq!(r.label_word.as_deref(), Some("Body"));
    }

    #[test]
    fn segment_header() {
        let r = n("Ver.R Hirabami F-M -1- Helm-00-segment");
        assert_eq!(r.group_index, Some(1));
        // "00" is numeric, but role (Info) is decided later from payload absence.
        assert_eq!(r.slot_token.as_deref(), Some("00"));
    }

    #[test]
    fn basic_must_install_has_group_no_slot() {
        let r = n("Ver.R Hirabami F-M -0- basic files (must install)");
        assert_eq!(r.group_index, Some(0));
        assert_eq!(r.slot_token, None);
    }

    #[test]
    fn radio_sets_split_physics_but_merge_armor_variants() {
        let helm1 = parse_folder("Ver.R Hirabami F-M -1- Helm-01-vanilla");
        let helm6 = parse_folder("Ver.R Hirabami F-M -1- Helm-06-bangs2 no headpiece");
        assert_eq!(
            radio_set_key(helm1.group_index, &helm1.body),
            radio_set_key(helm6.group_index, &helm6.body),
            "all Helm variants belong to one radio set"
        );

        let body = parse_folder("Ver.R Hirabami F-M -7- Physics-body-heavy");
        let leg = parse_folder("Ver.R Hirabami F-M -7- Physics-leg-none");
        assert_ne!(
            radio_set_key(body.group_index, &body.body),
            radio_set_key(leg.group_index, &leg.body),
            "Physics body and leg are independent radio sets"
        );

        let body2 = parse_folder("Ver.R Hirabami F-M -7- Physics-body-lite");
        assert_eq!(
            radio_set_key(body.group_index, &body.body),
            radio_set_key(body2.group_index, &body2.body),
            "Physics-body variants share one radio set"
        );
    }

    #[test]
    fn cover_and_warning() {
        assert_eq!(n("Ver.R Hirabami F-M -0- 0- cover -0").slot_token.as_deref(), Some("cover"));
        assert_eq!(n("Ver.R Hirabami F-M -0- 0- warning -0").slot_token.as_deref(), Some("warning"));
    }
}
