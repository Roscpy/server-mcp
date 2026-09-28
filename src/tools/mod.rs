pub mod fs_tools;

use anyhow::{bail, Result};
use std::path::{Component, Path, PathBuf};

/// Résout `requested` sous `root` et garantit que le résultat reste dans `root`
/// (refuse `..`, suit les liens symboliques via le plus proche ancêtre existant).
pub fn confine_path(root: &Path, requested: &str) -> Result<PathBuf> {
    let rel = Path::new(requested.trim_start_matches('/'));
    if rel
        .components()
        .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
    {
        bail!("chemin refusé: '{}' contient '..'", requested);
    }

    let canon_root = root.canonicalize()?;
    let candidate = canon_root.join(rel);

    let mut existing = candidate.as_path();
    let mut tail = Vec::new();
    while existing.symlink_metadata().is_err() {
        match (existing.file_name(), existing.parent()) {
            (Some(name), Some(parent)) => {
                tail.push(name.to_owned());
                existing = parent;
            }
            _ => bail!("chemin invalide: '{}'", requested),
        }
    }

    let mut resolved = existing.canonicalize()?;
    if !resolved.starts_with(&canon_root) {
        bail!("chemin refusé: '{}' sort de la racine autorisée", requested);
    }
    for name in tail.into_iter().rev() {
        resolved.push(name);
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_root(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("mcp-vps-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn rejects_parent_dir() {
        let root = tmp_root("a");
        assert!(confine_path(&root, "../../etc/passwd").is_err());
        assert!(confine_path(&root, "a/../../x/new.txt").is_err());
    }

    #[test]
    fn accepts_new_nested_path() {
        let root = tmp_root("b");
        let p = confine_path(&root, "new/dir/file.txt").unwrap();
        assert!(p.starts_with(root.canonicalize().unwrap()));
    }
}
