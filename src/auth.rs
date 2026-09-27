//! Vérification du header `Authorization: Bearer <token>` pour le
//! transport HTTP/SSE distant. En mode stdio (local), l'auth est implicite
//! (le processus est lancé directement par le client MCP), donc ce module
//! n'est utilisé que par la branche http de main.rs.

/// Comparaison en temps constant pour éviter le timing attack sur le token.
pub fn tokens_match(provided: &str, expected: &str) -> bool {
    let a = provided.as_bytes();
    let b = expected.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

pub fn extract_bearer(header_value: Option<&str>) -> Option<&str> {
    header_value?.strip_prefix("Bearer ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_token() {
        assert_eq!(extract_bearer(Some("Bearer abc123")), Some("abc123"));
        assert_eq!(extract_bearer(Some("Basic xyz")), None);
        assert_eq!(extract_bearer(None), None);
    }

    #[test]
    fn constant_time_compare() {
        assert!(tokens_match("secret", "secret"));
        assert!(!tokens_match("secret", "wrong"));
        assert!(!tokens_match("short", "longer-token"));
    }
}
