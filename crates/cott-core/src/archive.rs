//! `.ctgdaw` project archive format (ZIP with a fixed internal layout).
//!
//! Layout:
//! ```text
//! project.ctgdaw
//!   project.json   # versioned Project manifest
//!   assets/...     # imported media (paths match Asset.relative_path)
//! ```

use crate::project::{PROJECT_VERSION, Project, ProjectError};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

pub const ARCHIVE_EXTENSION: &str = "ctgdaw";
pub const MANIFEST_NAME: &str = "project.json";

/// Validate a relative path that will be stored inside or extracted from an archive.
///
/// Rejects absolute paths, parent-directory traversal, and empty components.
pub fn validate_archive_path(path: &Path) -> Result<(), ProjectError> {
    if path.as_os_str().is_empty() {
        return Err(ProjectError::Invalid("empty archive path".into()));
    }
    if path.is_absolute() {
        return Err(ProjectError::Invalid(format!(
            "absolute path not allowed in archive: {}",
            path.display()
        )));
    }
    let mut has_normal = false;
    for component in path.components() {
        match component {
            Component::Normal(_) => has_normal = true,
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(ProjectError::Invalid(format!(
                    "parent traversal not allowed in archive path: {}",
                    path.display()
                )));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(ProjectError::Invalid(format!(
                    "absolute path not allowed in archive: {}",
                    path.display()
                )));
            }
        }
    }
    if !has_normal {
        return Err(ProjectError::Invalid(format!(
            "invalid archive path: {}",
            path.display()
        )));
    }
    Ok(())
}

/// Normalize a ZIP entry name to a relative [`PathBuf`] with `/` separators validated.
pub fn sanitize_zip_entry_name(name: &str) -> Result<PathBuf, ProjectError> {
    if name.is_empty() {
        return Err(ProjectError::Invalid("empty zip entry name".into()));
    }
    // Reject absolute / drive paths before stripping separators.
    if name.starts_with('/')
        || name.starts_with('\\')
        || (name.len() >= 2 && name.as_bytes()[1] == b':')
    {
        return Err(ProjectError::Invalid(format!(
            "absolute path not allowed in archive: {name}"
        )));
    }
    let trimmed = name.trim_matches(|c| c == '/' || c == '\\');
    if trimmed.is_empty() {
        return Err(ProjectError::Invalid("empty zip entry name".into()));
    }
    let path = PathBuf::from(trimmed.replace('\\', "/"));
    validate_archive_path(&path)?;
    Ok(path)
}

/// Resolve `relative` under `root`, ensuring the result stays inside `root`.
pub fn safe_join(root: &Path, relative: &Path) -> Result<PathBuf, ProjectError> {
    validate_archive_path(relative)?;
    let joined = root.join(relative);
    let root_canon = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    // Parent may not exist yet; canonicalize what we can.
    let parent = joined.parent().unwrap_or(root);
    if parent.exists() {
        let parent_canon = parent.canonicalize().map_err(|e| {
            ProjectError::Invalid(format!(
                "failed to resolve archive path {}: {e}",
                relative.display()
            ))
        })?;
        if !parent_canon.starts_with(&root_canon) {
            return Err(ProjectError::Invalid(format!(
                "archive path escapes workspace: {}",
                relative.display()
            )));
        }
    }
    Ok(joined)
}

/// Pack a project workspace directory into a `.ctgdaw` ZIP archive.
///
/// Writes to a sibling temporary file then atomically renames onto `archive_path`.
pub fn pack_workspace(
    project: &Project,
    workspace: &Path,
    archive_path: &Path,
) -> Result<(), ProjectError> {
    if let Some(parent) = archive_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file_name = archive_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project.ctgdaw");
    let tmp_path = archive_path.with_file_name(format!("{file_name}.tmp"));

    {
        let file = File::create(&tmp_path)?;
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        // Always write the current in-memory manifest (may include freshly flushed plugin state).
        let mut project_for_json = project.clone();
        project_for_json.meta.version = PROJECT_VERSION;
        let json = serde_json::to_string_pretty(&project_for_json)?;
        zip.start_file(MANIFEST_NAME, options)?;
        zip.write_all(json.as_bytes())?;

        // Bundle registered asset files from the workspace.
        for asset in project.assets.values() {
            validate_archive_path(&asset.relative_path)?;
            let src = workspace.join(&asset.relative_path);
            if !src.exists() {
                continue;
            }
            let mut file = File::open(&src)?;
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)?;
            let entry_name = asset
                .relative_path
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            zip.start_file(&entry_name, options)?;
            zip.write_all(&buf)?;
        }

        zip.finish()?;
    }

    if let Err(rename_err) = std::fs::rename(&tmp_path, archive_path) {
        if let Err(copy_err) = std::fs::copy(&tmp_path, archive_path) {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(copy_err.into());
        }
        let _ = std::fs::remove_file(&tmp_path);
        // Prefer rename error only when copy also failed; rename often fails across devices.
        let _ = rename_err;
    }
    Ok(())
}

/// Extract a `.ctgdaw` archive into `workspace` and return the deserialized project.
pub fn unpack_archive(archive_path: &Path, workspace: &Path) -> Result<Project, ProjectError> {
    std::fs::create_dir_all(workspace)?;
    std::fs::create_dir_all(workspace.join("assets"))?;

    let file = File::open(archive_path)?;
    let mut zip = ZipArchive::new(file).map_err(|e| {
        ProjectError::Invalid(format!(
            "invalid .ctgdaw archive {}: {e}",
            archive_path.display()
        ))
    })?;

    let mut manifest_bytes: Option<Vec<u8>> = None;

    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| ProjectError::Invalid(format!("failed to read zip entry: {e}")))?;
        let raw_name = entry.name().to_string();
        if raw_name.ends_with('/') {
            let rel = sanitize_zip_entry_name(raw_name.trim_end_matches('/'))?;
            let dir = safe_join(workspace, &rel)?;
            std::fs::create_dir_all(&dir)?;
            continue;
        }

        let rel = sanitize_zip_entry_name(&raw_name)?;
        if rel.as_os_str() == MANIFEST_NAME {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            manifest_bytes = Some(buf);
            continue;
        }

        let dest = safe_join(workspace, &rel)?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = File::create(&dest)?;
        std::io::copy(&mut entry, &mut out)?;
    }

    let Some(bytes) = manifest_bytes else {
        return Err(ProjectError::Invalid(
            "archive missing project.json manifest".into(),
        ));
    };

    let mut project: Project = serde_json::from_slice(&bytes)?;
    if project.meta.version > PROJECT_VERSION {
        return Err(ProjectError::UnsupportedVersion(project.meta.version));
    }
    if project.meta.version < PROJECT_VERSION {
        project.meta.version = PROJECT_VERSION;
    }
    project.root_dir = Some(workspace.to_path_buf());

    for asset in project.assets.values_mut() {
        validate_archive_path(&asset.relative_path)?;
        let full = workspace.join(&asset.relative_path);
        asset.missing = !full.exists();
    }

    // Also write project.json into the workspace for tooling / legacy helpers.
    let manifest_path = workspace.join(MANIFEST_NAME);
    std::fs::write(&manifest_path, &bytes)?;

    Ok(project)
}

/// Copy a legacy directory project (`project.json` + `assets/`) into a fresh workspace.
pub fn copy_legacy_project_into(src_dir: &Path, workspace: &Path) -> Result<Project, ProjectError> {
    let mut project = Project::load_from_dir(src_dir)?;
    std::fs::create_dir_all(workspace)?;
    std::fs::create_dir_all(workspace.join("assets"))?;

    // Copy manifest.
    let src_manifest = src_dir.join(MANIFEST_NAME);
    if src_manifest.exists() {
        std::fs::copy(&src_manifest, workspace.join(MANIFEST_NAME))?;
    }

    // Copy registered assets (and any present files under assets/).
    for asset in project.assets.values() {
        validate_archive_path(&asset.relative_path)?;
        let src = src_dir.join(&asset.relative_path);
        let dest = safe_join(workspace, &asset.relative_path)?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if src.exists() {
            std::fs::copy(&src, &dest)?;
        }
    }

    project.root_dir = Some(workspace.to_path_buf());
    for asset in project.assets.values_mut() {
        let full = workspace.join(&asset.relative_path);
        asset.missing = !full.exists();
    }
    Ok(project)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::AssetId;
    use crate::project::{Asset, AssetKind};
    use tempfile::tempdir;

    #[test]
    fn rejects_parent_traversal() {
        assert!(validate_archive_path(Path::new("../secret")).is_err());
        assert!(sanitize_zip_entry_name("../../etc/passwd").is_err());
        assert!(sanitize_zip_entry_name("/etc/passwd").is_err());
    }

    #[test]
    fn accepts_normal_asset_paths() {
        validate_archive_path(Path::new("assets/beep.wav")).unwrap();
        sanitize_zip_entry_name("assets/beep.wav").unwrap();
        sanitize_zip_entry_name("project.json").unwrap();
    }

    #[test]
    fn pack_unpack_roundtrip_preserves_assets() {
        let workspace = tempdir().unwrap();
        let assets = workspace.path().join("assets");
        std::fs::create_dir_all(&assets).unwrap();
        let wav = assets.join("beep.wav");
        let payload = b"RIFF....WAVEfmt asset-bytes";
        std::fs::write(&wav, payload).unwrap();

        let mut project = Project::new("Archive");
        project.root_dir = Some(workspace.path().to_path_buf());
        let id = AssetId::new();
        project.assets.insert(
            id,
            Asset {
                id,
                name: "beep".into(),
                relative_path: PathBuf::from("assets/beep.wav"),
                kind: AssetKind::Audio,
                sample_rate: 48000,
                channels: 2,
                length_samples: 100,
                missing: false,
            },
        );

        let out = tempdir().unwrap();
        let archive = out.path().join("song.ctgdaw");
        pack_workspace(&project, workspace.path(), &archive).unwrap();
        assert!(archive.exists());

        let extract = tempdir().unwrap();
        let loaded = unpack_archive(&archive, extract.path()).unwrap();
        assert_eq!(loaded.meta.name, "Archive");
        assert_eq!(loaded.assets.len(), 1);
        let restored = extract.path().join("assets/beep.wav");
        assert_eq!(std::fs::read(&restored).unwrap(), payload);
        assert!(!loaded.assets.values().next().unwrap().missing);
    }

    #[test]
    fn unpack_rejects_zip_slip() {
        use std::io::Write;
        let dir = tempdir().unwrap();
        let evil = dir.path().join("evil.ctgdaw");
        {
            let file = File::create(&evil).unwrap();
            let mut zip = ZipWriter::new(file);
            let options =
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zip.start_file(MANIFEST_NAME, options).unwrap();
            let project = Project::new("Evil");
            let json = serde_json::to_string_pretty(&project).unwrap();
            zip.write_all(json.as_bytes()).unwrap();
            zip.start_file("../outside.txt", options).unwrap();
            zip.write_all(b"pwned").unwrap();
            zip.finish().unwrap();
        }
        let extract = tempdir().unwrap();
        let err = unpack_archive(&evil, extract.path()).unwrap_err();
        assert!(
            matches!(err, ProjectError::Invalid(_)),
            "expected Invalid, got {err:?}"
        );
    }

    #[test]
    fn copy_legacy_project_into_workspace() {
        let legacy = tempdir().unwrap();
        let mut project = Project::new("Legacy");
        project.save_to_dir(legacy.path()).unwrap();
        let wav = legacy.path().join("assets/tone.wav");
        std::fs::write(&wav, b"legacy-wav").unwrap();
        let id = AssetId::new();
        project.assets.insert(
            id,
            Asset {
                id,
                name: "tone".into(),
                relative_path: PathBuf::from("assets/tone.wav"),
                kind: AssetKind::Audio,
                sample_rate: 48000,
                channels: 2,
                length_samples: 50,
                missing: false,
            },
        );
        // Persist asset registration into the legacy folder.
        project.save_to_dir(legacy.path()).unwrap();

        let workspace = tempdir().unwrap();
        let loaded = copy_legacy_project_into(legacy.path(), workspace.path()).unwrap();
        assert_eq!(loaded.meta.name, "Legacy");
        assert_eq!(
            std::fs::read(workspace.path().join("assets/tone.wav")).unwrap(),
            b"legacy-wav"
        );
        assert_eq!(loaded.root_dir.as_deref(), Some(workspace.path()));
    }
}
