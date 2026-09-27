pub mod fs_tools;

use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

/// Résout `requested` relativement à `root` et vérifie que le résultat
/// reste bien à l'intérieur de `root`, même via `..`, liens symboliques
/// ou chemins absolus fournis par erreur/malice.
///
/// C'est la seule porte d'entrée que les tools filesystem doivent utiliser
/// pour toucher au disque — ne jamais joindre un chemin utilisateur
/// directement avec `root`.
pub fn confine_path(root: &Path, requested: &str) -> Result<PathBuf> {
    let candidate = root.join(requested.trim_start_matches('/'));

    // canonicalize exige que le chemin existe déjà pour les lectures ;
    // pour les écritures (fichier pas encore créé), on canonicalise le
    // dossier parent puis on rajoute le nom de fichier.
    let (parent_to_check, file_name) = if candidate.exists() {
        (candidate.clone(), None)
    } else {
        let parent = candidate.parent().unwrap_or(root).to_path_buf();
        let name = candidate.file_name().map(|f| f.to_owned());
        (parent, name)
    };

    let canon_root = root.canonicalize()?;
    let canon_parent = parent_to_check
        .canonicalize()
        .unwrap_or_else(|_| parent_to_check.clone());

    if !canon_parent.starts_with(&canon_root) {
        bail!(
            "chemin refusé: '{}' sort de la racine autorisée",
            requested
        );
    }

    Ok(match file_name {
        Some(name) => canon_parent.join(name),
        None => canon_parent,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_path_traversal() {
        let tmp = std::env::temp_dir().join("mcp-vps-test-root");
        std::fs::create_dir_all(&tmp).unwrap();
        let res = confine_path(&tmp, "../../etc/passwd");
        assert!(res.is_err());
    }
}
