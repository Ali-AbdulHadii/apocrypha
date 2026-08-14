//! Staging: extract a mod's payload files into the staging library.
//!
//! Staged mods live **outside the game directory** (`.../games/<game>/staging/<id>/payload/`),
//! namespaced per option so any later selection can be deployed without needing
//! the original archive again:
//!
//! ```text
//! payload/<option_folder>/natives/stm/.../ch03_023_0003.mesh.241111606
//! payload/<option_folder>/reframework/data/.../file.json
//! ```
//!
//! Only payload files (`natives/`, `reframework/`) are extracted; screenshots and
//! `modinfo.ini` are metadata already captured in the [`ModBundle`].

use crate::error::{ModEngineError, Result};
use crate::plan::staged_rel_path;
use apoc_domain::ModBundle;
use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Outcome of staging a mod archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageReport {
    pub payload_root: PathBuf,
    pub files_written: usize,
    pub bytes_written: u64,
    /// Preview images extracted alongside the payload.
    pub previews_written: usize,
}

/// Subdirectory of a mod's staging root holding option preview images.
pub const PREVIEW_DIR: &str = ".previews";

/// Staged path of an option's preview image, given its original filename.
pub fn preview_rel_path(option_id: &str, screenshot: &str) -> String {
    let ext = Path::new(screenshot)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("jpg");
    format!(
        "{}/{}.{}",
        PREVIEW_DIR,
        crate::plan::option_dir(option_id),
        ext
    )
}

/// Locate an option's staged preview image, if one was extracted.
pub fn staged_preview(staging_root: &Path, option_id: &str, screenshot: &str) -> Option<PathBuf> {
    let p = staging_root.join(preview_rel_path(option_id, screenshot));
    p.is_file().then_some(p)
}

/// Read a single entry out of an archive. Used to show option previews before
/// a mod has been imported (the wizard runs against the archive directly).
pub fn read_archive_entry(archive_path: &Path, entry_path: &str) -> Result<Vec<u8>> {
    crate::archive::read_entry(archive_path, entry_path)
}

/// Reject absolute paths and `..` traversal (zip-slip) before writing anything.
fn safe_join(root: &Path, rel: &str) -> Result<PathBuf> {
    let candidate = Path::new(rel);
    if candidate.is_absolute() {
        return Err(ModEngineError::UnsafePath(rel.to_string()));
    }
    let mut out = root.to_path_buf();
    for comp in candidate.components() {
        match comp {
            Component::Normal(seg) => out.push(seg),
            Component::CurDir => {}
            _ => return Err(ModEngineError::UnsafePath(rel.to_string())),
        }
    }
    if !out.starts_with(root) {
        return Err(ModEngineError::UnsafePath(rel.to_string()));
    }
    Ok(out)
}

/// Extract every deployable payload file of `bundle` from `archive_path` into
/// `dest_root`, laid out as `<dest_root>/<option_folder>/<game_rel_path>`.
///
/// Existing files are overwritten, so re-staging is idempotent.
pub fn stage_bundle(
    archive_path: &Path,
    bundle: &ModBundle,
    dest_root: &Path,
) -> Result<StageReport> {
    // archive entry path -> every staged relative path that wants those bytes.
    //
    // Many destinations per entry, because a manifest-driven installer can cite
    // one source from two options, or map one file to two places. Under the
    // folder conventions this predates, an entry belonged to exactly one option
    // and a single destination was safe; with one it is not, and the loss is
    // silent.
    let mut wanted: HashMap<String, Vec<String>> = HashMap::new();
    for opt in bundle.deployable_options() {
        for f in &opt.payload {
            let rel = staged_rel_path(&opt.id, &f.game_rel_path);
            let dests = wanted.entry(f.archive_path.clone()).or_default();
            // One option declaring the same destination twice writes it once.
            if !dests.contains(&rel) {
                dests.push(rel);
            }
        }
    }
    if wanted.is_empty() {
        return Err(ModEngineError::Empty);
    }

    // Preview images for every option that declares one: including info-only
    // options (cover art, warnings), which carry no payload but are the images
    // the wizard most needs to show.
    // Two options may well name the same image, which is another reason one
    // destination per entry was never enough.
    for opt in bundle.options() {
        if let (Some(shot), Some(path)) = (&opt.screenshot, &opt.screenshot_archive_path) {
            let rel = preview_rel_path(&opt.id, shot);
            let dests = wanted.entry(path.clone()).or_default();
            if !dests.contains(&rel) {
                dests.push(rel);
            }
        }
    }

    fs::create_dir_all(dest_root)?;

    // Traversal is rejected here, at the moment a destination is resolved, so
    // no format-specific extractor can be tricked into writing outside staging.
    let written = crate::archive::extract_entries(archive_path, &wanted, &mut |rel| {
        safe_join(dest_root, rel)
    })?;

    let mut files_written = 0usize;
    let mut previews_written = 0usize;
    let mut bytes_written = 0u64;

    for (rel, n) in written {
        if rel.starts_with(PREVIEW_DIR) {
            previews_written += 1;
        } else {
            files_written += 1;
        }
        bytes_written += n;
    }

    Ok(StageReport {
        payload_root: dest_root.to_path_buf(),
        files_written,
        bytes_written,
        previews_written,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal_and_absolute_paths() {
        let root = Path::new("/tmp/stage-root");
        assert!(safe_join(root, "../escape").is_err());
        assert!(safe_join(root, "/etc/passwd").is_err());
        assert!(safe_join(root, "a/../../b").is_err());
        assert!(safe_join(root, "opt/natives/ok.mesh.1").is_ok());
    }

    #[test]
    fn preserves_versioned_extensions_verbatim() {
        let root = Path::new("/tmp/stage-root");
        let p = safe_join(root, "Helm-01/natives/stm/x.mesh.241111606").unwrap();
        assert!(p.to_string_lossy().ends_with("x.mesh.241111606"));
    }

    #[test]
    fn one_archive_entry_reaches_every_option_that_asked_for_it() {
        use apoc_domain::{
            DeployRoot, FilePayload, InstallerModel, ModBundle, ModOption, OptionGroup, SelectMode,
        };
        use std::io::Write;

        // Two options citing the same source file. Impossible under the folder
        // conventions, where each option owned its own directory; routine for a
        // manifest, where options name the files they want.
        fn option(id: &str, dest: &str) -> ModOption {
            ModOption {
                id: id.to_string(),
                folder_name: id.to_string(),
                group_index: None,
                slot_token: None,
                radio_set: None,
                name: id.to_string(),
                description: None,
                category: None,
                author: None,
                screenshot: None,
                screenshot_archive_path: None,
                select_mode: SelectMode::Stackable,
                recommended: false,
                blocked_reason: None,
                deployable: true,
                payload: vec![FilePayload {
                    archive_path: "shared/texture.dds".into(),
                    game_rel_path: dest.into(),
                    root: DeployRoot::GameRoot,
                    size: 4,
                    priority: 0,
                }],
                raw_modinfo: Default::default(),
            }
        }

        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("mod.zip");
        {
            let f = std::fs::File::create(&archive).unwrap();
            let mut zip = zip::ZipWriter::new(f);
            let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zip.start_file("shared/texture.dds", opts).unwrap();
            zip.write_all(b"DDS!").unwrap();
            zip.finish().unwrap();
        }

        let bundle = ModBundle {
            name: "Shared".into(),
            version: None,
            author: None,
            category: None,
            installer_model: InstallerModel::Fomod,
            archive_sha256: None,
            fomod: None,
            groups: vec![OptionGroup {
                index: None,
                label: "G".into(),
                cardinality: None,
                options: vec![option("first", "a/tex.dds"), option("second", "b/tex.dds")],
            }],
        };

        let staged = tmp.path().join("staging");
        let report = stage_bundle(&archive, &bundle, &staged).unwrap();

        assert_eq!(
            report.files_written, 2,
            "both options must be staged, not just whichever was recorded last"
        );
        assert!(staged.join("first/a/tex.dds").is_file());
        assert!(staged.join("second/b/tex.dds").is_file());
    }
}
