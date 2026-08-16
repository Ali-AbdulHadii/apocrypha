//! Reading and writing a game's plugin list.
//!
//! The first thing this crate writes that is not in the game directory. The
//! list lives in the Proton prefix, under
//! `compatdata/<appid>/pfx/drive_c/users/steamuser/AppData/Local/<dir>/`, and
//! issue #16 asked what the three safety invariants mean somewhere the game
//! install is not.
//!
//! **They apply, unchanged in substance.** The invariants were never about
//! which directory a file is in. They are about the order of operations in a
//! directory this application does not own, and every reason they exist holds
//! here more strongly than it does for a game file:
//!
//! 1. **Vault before overwrite.** The original list goes into the same
//!    content-addressed vault, keyed by its own hash, before a byte is written.
//!    The only widening is in the wording — "no pre-existing *game* file" was
//!    describing the only files that existed to replace, not carving out an
//!    exemption for later ones.
//! 2. **Journal before it counts.** The write is recorded, flushed, and only
//!    then does anything else proceed.
//! 3. **Hash-guarded restore.** Rollback re-hashes before putting the original
//!    back, and a file that no longer matches what we wrote is left alone and
//!    reported. This is the invariant with the strongest claim on a plugin
//!    list: a game file is edited by Steam and by other mod managers, but a
//!    plugin list is edited by *the user*, deliberately, in the tool they
//!    already use. Overwriting that is the failure `unmanaged_plugin_notice`
//!    existed to avoid, and it would be a poor trade to remove the notice by
//!    committing the thing it warned about.
//!
//! The file is written the way `user.reg` is: to a temporary file, then
//! renamed, so a crash mid-write leaves the previous list intact rather than
//! half of two lists.

use crate::journal::JournalOp;
use crate::vault::{self, Vault};
use crate::{DeployError, Result};
use apoc_domain::plugins::{PluginEntry, PluginOrder};
use apoc_domain::{PluginActivation, PluginListSpec};
use std::fs;
use std::path::{Path, PathBuf};

/// Where one game's list files are, resolved from the prefix and the profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginListTarget {
    /// The directory holding the files. Created if the game has not run yet.
    pub dir: PathBuf,
    /// Order and activation.
    pub plugins_file: PathBuf,
    /// Order alone, for the tools that still read it.
    pub load_order_file: Option<PathBuf>,
    pub activation: PluginActivation,
    /// Plugins the engine loads whether the list names them or not.
    pub implicit: Vec<String>,
}

impl PluginListTarget {
    /// Resolve a profile's list against a Proton prefix.
    ///
    /// `prefix` is the `pfx` directory. Returns `None` when the profile does
    /// not manage a list, or when the spec names a directory that is not a
    /// plain name — a published profile decides this string, and joining `..`
    /// onto a path is how it would choose to write somewhere else.
    pub fn resolve(prefix: &Path, spec: &PluginListSpec) -> Option<Self> {
        if !is_plain_name(&spec.dir) || !is_plain_name(&spec.plugins_file) {
            return None;
        }
        if let Some(name) = &spec.load_order_file {
            if !is_plain_name(name) {
                return None;
            }
        }

        let dir = prefix
            .join("drive_c/users/steamuser/AppData/Local")
            .join(&spec.dir);
        Some(PluginListTarget {
            plugins_file: dir.join(&spec.plugins_file),
            load_order_file: spec.load_order_file.as_ref().map(|n| dir.join(n)),
            dir,
            activation: spec.activation,
            implicit: spec.implicit.clone(),
        })
    }
}

/// One path component, and an ordinary one.
fn is_plain_name(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && !s.contains('/')
        && !s.contains('\\')
        && !s.contains('\0')
}

/// Read the list the user already has.
///
/// A missing file is an empty order, not an error: it means the game has not
/// run yet, which is ordinary on a fresh install. Everything found is adopted
/// verbatim — this never sorts, dedupes into a different order, or drops a line
/// it did not understand beyond a comment.
pub fn read(target: &PluginListTarget) -> PluginOrder {
    let text = fs::read_to_string(&target.plugins_file).unwrap_or_default();
    let mut entries: Vec<PluginEntry> = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        // `#` is the comment convention these files use, and a blank line
        // carries nothing.
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (enabled, name) = match target.activation {
            PluginActivation::Asterisk => match line.strip_prefix('*') {
                Some(rest) => (true, rest.trim()),
                None => (false, line),
            },
            // Being in the file is what activation means here.
            PluginActivation::Presence => (true, line),
        };
        if name.is_empty() {
            continue;
        }

        let mut entry = PluginEntry::unknown(name);
        entry.enabled = enabled;
        entry.implicit = target.implicit.iter().any(|i| i.eq_ignore_ascii_case(name));
        entries.push(entry);
    }

    // The engine loads these whether they are written down or not, so a list
    // that never mentioned them still has them.
    //
    // Each missing one is inserted after the implicit entries already present,
    // rather than at the front: the declared order is the game's, but the file's
    // order is the user's, and prepending would put a newly-added `Update.esm`
    // ahead of a `Skyrim.esm` that was already written down. Nothing already in
    // the list moves.
    for name in &target.implicit {
        if entries.iter().any(|e| e.name.eq_ignore_ascii_case(name)) {
            continue;
        }
        let at = entries
            .iter()
            .rposition(|e| e.implicit)
            .map(|last| last + 1)
            .unwrap_or(0);
        entries.insert(
            at,
            PluginEntry {
                enabled: true,
                implicit: true,
                ..PluginEntry::unknown(name.clone())
            },
        );
    }

    PluginOrder::adopt(entries)
}

/// What one list file should contain.
fn render(order: &PluginOrder, activation: PluginActivation, with_markers: bool) -> String {
    let mut out = String::new();
    for entry in order.as_the_game_reads_it() {
        match (with_markers, activation) {
            (true, PluginActivation::Asterisk) => {
                if entry.enabled {
                    out.push('*');
                }
                out.push_str(&entry.name);
            }
            // Presence is activation: a disabled plugin is simply not written.
            (true, PluginActivation::Presence) => {
                if !entry.enabled {
                    continue;
                }
                out.push_str(&entry.name);
            }
            // `loadorder.txt` is the order alone, every plugin, no markers.
            (false, _) => out.push_str(&entry.name),
        }
        out.push('\n');
    }
    out
}

/// Write the list, vaulting whatever was there first.
///
/// Returns the journal operations describing what was done, for the caller to
/// append **before** treating any of it as having happened. Nothing here writes
/// the journal itself, so the ordering of the second invariant stays visible at
/// the call site rather than buried in here.
///
/// A file whose contents would not change is not written at all: a deploy that
/// touches nothing should leave no trace, and an unchanged list that still gets
/// a new mtime is one more thing to explain when somebody is looking for what
/// went wrong.
pub fn write(
    target: &PluginListTarget,
    vault_dir: &Path,
    order: &PluginOrder,
) -> Result<Vec<JournalOp>> {
    let vault = Vault::new(vault_dir);
    let mut ops = Vec::new();

    let files: Vec<(&PathBuf, String)> =
        std::iter::once((&target.plugins_file, render(order, target.activation, true)))
            .chain(
                target
                    .load_order_file
                    .as_ref()
                    .map(|p| (p, render(order, target.activation, false))),
            )
            .collect();

    for (path, contents) in files {
        if let Some(op) = write_one(&vault, path, &contents)? {
            ops.push(op);
        }
    }
    Ok(ops)
}

fn write_one(vault: &Vault, path: &Path, contents: &str) -> Result<Option<JournalOp>> {
    if let Ok(existing) = fs::read_to_string(path) {
        if existing == contents {
            return Ok(None);
        }
    }

    // Invariant 1. The original's bytes are in the vault before anything is
    // written, so a crash between the two steps still has something to restore.
    let original_vault_key = if path.exists() {
        Some(vault.store(path).map_err(DeployError::Io)?)
    } else {
        None
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(DeployError::Io)?;
    }
    // Temp file then rename, as `user.reg` is written: a crash mid-write leaves
    // the previous list whole rather than half of two lists.
    let tmp = path.with_extension("apoc-tmp");
    fs::write(&tmp, contents).map_err(DeployError::Io)?;
    fs::rename(&tmp, path).map_err(DeployError::Io)?;

    let sha256 = vault::hash_file(path).map_err(DeployError::Io)?;
    Ok(Some(JournalOp::PluginListWritten {
        path: path.to_string_lossy().into_owned(),
        original_vault_key,
        sha256,
    }))
}

/// Put back the list we found, if it is still the one we wrote.
///
/// Invariant 3, and the case it matters most for: between the deploy and the
/// rollback the user may have opened their load-order tool and curated the
/// list. A mismatch means exactly that, and the file is left alone.
///
/// Returns `Ok(true)` when the original was restored or our file removed, and
/// `Ok(false)` when it was left because somebody else had written it.
pub fn rollback_one(
    path: &Path,
    original_vault_key: Option<&str>,
    sha256: &str,
    vault_dir: &Path,
) -> Result<bool> {
    if !path.exists() {
        // Already gone. Nothing to guard and nothing to undo.
        return Ok(true);
    }
    let current = vault::hash_file(path).map_err(DeployError::Io)?;
    if current != sha256 {
        return Ok(false);
    }

    match original_vault_key {
        Some(key) => Vault::new(vault_dir)
            .restore(key, path)
            .map(|_| true)
            .map_err(DeployError::Io),
        // There was no list before this deploy, so the state we found is one
        // where the file did not exist.
        None => fs::remove_file(path).map(|_| true).map_err(DeployError::Io),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apoc_domain::plugins::PluginKind;
    use tempfile::TempDir;

    fn spec() -> PluginListSpec {
        PluginListSpec {
            dir: "Skyrim Special Edition".into(),
            plugins_file: "plugins.txt".into(),
            load_order_file: Some("loadorder.txt".into()),
            activation: PluginActivation::Asterisk,
            implicit: vec!["Skyrim.esm".into(), "Update.esm".into()],
        }
    }

    struct Fixture {
        _dir: TempDir,
        target: PluginListTarget,
        vault_dir: PathBuf,
    }

    fn fixture() -> Fixture {
        let dir = TempDir::new().unwrap();
        let prefix = dir.path().join("pfx");
        let target = PluginListTarget::resolve(&prefix, &spec()).expect("a plain spec resolves");
        fs::create_dir_all(&target.dir).unwrap();
        Fixture {
            vault_dir: dir.path().join("vault"),
            _dir: dir,
            target,
        }
    }

    fn plugin(name: &str, kind: PluginKind, enabled: bool) -> PluginEntry {
        PluginEntry {
            name: name.into(),
            enabled,
            kind,
            masters: Vec::new(),
            implicit: false,
        }
    }

    #[test]
    fn a_directory_that_is_not_a_plain_name_does_not_resolve() {
        let prefix = Path::new("/prefix");
        for bad in ["..", "../../etc", "a/b", "", "."] {
            let spec = PluginListSpec {
                dir: bad.into(),
                ..spec()
            };
            assert!(
                PluginListTarget::resolve(prefix, &spec).is_none(),
                "{bad:?} should not resolve"
            );
        }
    }

    #[test]
    fn a_missing_list_reads_as_the_plugins_the_engine_loads_anyway() {
        let f = fixture();
        let order = read(&f.target);

        let names: Vec<&str> = order.entries().iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["Skyrim.esm", "Update.esm"]);
        assert!(order.entries().iter().all(|e| e.implicit));
    }

    #[test]
    fn the_users_own_order_is_read_back_exactly_as_written() {
        let f = fixture();
        fs::write(
            &f.target.plugins_file,
            "# a comment\n\
             *Skyrim.esm\n\
             *Update.esm\n\
             *ZLast.esp\n\
             ADisabled.esp\n\
             \n\
             *BMiddle.esp\n",
        )
        .unwrap();

        let order = read(&f.target);
        let names: Vec<&str> = order.entries().iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "Skyrim.esm",
                "Update.esm",
                "ZLast.esp",
                "ADisabled.esp",
                "BMiddle.esp"
            ],
            "adopted in file order, not sorted"
        );
        assert!(!order.get("ADisabled.esp").unwrap().enabled);
        assert!(order.get("BMiddle.esp").unwrap().enabled);
        assert!(order.get("Skyrim.esm").unwrap().implicit);
    }

    #[test]
    fn presence_activation_reads_and_writes_without_markers() {
        let f = fixture();
        let target = PluginListTarget {
            activation: PluginActivation::Presence,
            ..f.target.clone()
        };
        fs::write(&target.plugins_file, "Skyrim.esm\nMod.esp\n").unwrap();

        let order = read(&target);
        assert!(order.get("Mod.esp").unwrap().enabled, "presence is on");

        let rendered = render(&order, PluginActivation::Presence, true);
        assert!(!rendered.contains('*'));
        assert!(rendered.contains("Mod.esp"));
    }

    #[test]
    fn writing_vaults_the_original_first_and_journals_what_it_did() {
        let f = fixture();
        fs::write(&f.target.plugins_file, "*Skyrim.esm\n*Curated.esp\n").unwrap();
        let original = vault::hash_file(&f.target.plugins_file).unwrap();

        let mut order = read(&f.target);
        order.append_new([plugin("New.esp", PluginKind::Normal, true)]);
        let ops = write(&f.target, &f.vault_dir, &order).unwrap();

        // plugins.txt and loadorder.txt.
        assert_eq!(ops.len(), 2);
        let JournalOp::PluginListWritten {
            original_vault_key, ..
        } = &ops[0]
        else {
            panic!("expected a plugin list op, got {:?}", ops[0]);
        };
        assert_eq!(
            original_vault_key.as_deref(),
            Some(original.as_str()),
            "the original is in the vault under its own hash"
        );

        let written = fs::read_to_string(&f.target.plugins_file).unwrap();
        assert_eq!(
            written, "*Skyrim.esm\n*Update.esm\n*Curated.esp\n*New.esp\n",
            "Update.esm was missing from the file and the engine loads it anyway"
        );
        let load_order = fs::read_to_string(f.target.load_order_file.as_ref().unwrap()).unwrap();
        assert_eq!(
            load_order, "Skyrim.esm\nUpdate.esm\nCurated.esp\nNew.esp\n",
            "order alone, no markers"
        );
    }

    /// The declared order is the game's, but the file's order is the user's. A
    /// missing implicit master joins the ones already written down rather than
    /// jumping ahead of them.
    #[test]
    fn a_missing_implicit_master_is_inserted_without_moving_anything() {
        let f = fixture();
        fs::write(&f.target.plugins_file, "*Skyrim.esm\n*Curated.esp\n").unwrap();

        let order = read(&f.target);
        let names: Vec<&str> = order.entries().iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["Skyrim.esm", "Update.esm", "Curated.esp"]);
    }

    #[test]
    fn a_list_that_would_not_change_is_not_written_at_all() {
        let f = fixture();
        let order = read(&f.target);
        write(&f.target, &f.vault_dir, &order).unwrap();

        let ops = write(&f.target, &f.vault_dir, &order).unwrap();
        assert!(ops.is_empty(), "a no-op deploy leaves no trace");
    }

    #[test]
    fn the_master_block_is_written_first_whatever_order_the_list_is_in() {
        let f = fixture();
        let mut order = PluginOrder::adopt(vec![
            plugin("Normal.esp", PluginKind::Normal, true),
            plugin("Late.esm", PluginKind::Master, true),
        ]);
        order.append_new([plugin("Light.esl", PluginKind::LightMaster, true)]);

        write(&f.target, &f.vault_dir, &order).unwrap();

        let written = fs::read_to_string(&f.target.plugins_file).unwrap();
        assert_eq!(written, "*Late.esm\n*Light.esl\n*Normal.esp\n");
    }

    #[test]
    fn rollback_puts_the_original_back() {
        let f = fixture();
        fs::write(&f.target.plugins_file, "*Skyrim.esm\n*Curated.esp\n").unwrap();
        let mut order = read(&f.target);
        order.append_new([plugin("New.esp", PluginKind::Normal, true)]);
        let ops = write(&f.target, &f.vault_dir, &order).unwrap();

        let JournalOp::PluginListWritten {
            path,
            original_vault_key,
            sha256,
        } = &ops[0]
        else {
            panic!("expected a plugin list op");
        };

        let restored = rollback_one(
            Path::new(path),
            original_vault_key.as_deref(),
            sha256,
            &f.vault_dir,
        )
        .unwrap();

        assert!(restored);
        assert_eq!(
            fs::read_to_string(&f.target.plugins_file).unwrap(),
            "*Skyrim.esm\n*Curated.esp\n",
            "exactly what was there before"
        );
    }

    /// The reason invariant 3 matters more here than for a game file: between
    /// the deploy and the rollback, the person very likely opened the tool they
    /// curate this list in.
    #[test]
    fn a_list_the_user_edited_after_the_deploy_is_left_alone() {
        let f = fixture();
        fs::write(&f.target.plugins_file, "*Skyrim.esm\n").unwrap();
        let mut order = read(&f.target);
        order.append_new([plugin("New.esp", PluginKind::Normal, true)]);
        let ops = write(&f.target, &f.vault_dir, &order).unwrap();

        let JournalOp::PluginListWritten {
            path,
            original_vault_key,
            sha256,
        } = &ops[0]
        else {
            panic!("expected a plugin list op");
        };

        // The user reorders in their own tool.
        let theirs = "*Skyrim.esm\n*New.esp\n*SomethingElse.esp\n";
        fs::write(&f.target.plugins_file, theirs).unwrap();

        let restored = rollback_one(
            Path::new(path),
            original_vault_key.as_deref(),
            sha256,
            &f.vault_dir,
        )
        .unwrap();

        assert!(!restored, "reported, not overwritten");
        assert_eq!(
            fs::read_to_string(&f.target.plugins_file).unwrap(),
            theirs,
            "their curation survives the rollback"
        );
    }

    #[test]
    fn rollback_removes_a_list_that_did_not_exist_before_the_deploy() {
        let f = fixture();
        let mut order = read(&f.target);
        order.append_new([plugin("New.esp", PluginKind::Normal, true)]);
        let ops = write(&f.target, &f.vault_dir, &order).unwrap();

        let JournalOp::PluginListWritten {
            path,
            original_vault_key,
            sha256,
        } = &ops[0]
        else {
            panic!("expected a plugin list op");
        };
        assert!(original_vault_key.is_none(), "there was nothing to vault");

        assert!(rollback_one(Path::new(path), None, sha256, &f.vault_dir).unwrap());
        assert!(!f.target.plugins_file.exists());
    }
}
