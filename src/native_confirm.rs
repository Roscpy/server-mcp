//! Confirmation humaine via l'élicitation MCP native (popup côté client).
use rmcp::schemars::{self, JsonSchema};
use rmcp::service::{ElicitationError, Peer, RoleServer};
use serde::Deserialize;

// Formulaire plat à un seul booléen, comme l'exige le schéma d'élicitation MCP.
#[derive(Debug, Deserialize, JsonSchema)]
struct ApprovalForm {
    approve: bool,
}

rmcp::elicit_safe!(ApprovalForm);

pub enum Native {
    Approved,
    Denied(String),
    /// Le client ne supporte pas l'élicitation : repli sur Telegram.
    Unsupported,
}

pub async fn ask(peer: &Peer<RoleServer>, command: &str, remember: bool) -> Native {
    let mut message = format!("Exécuter cette commande hors liste blanche ? {command}");
    if remember {
        message.push_str("\n(Approuver l'ajoutera définitivement à la liste blanche.)");
    }
    match peer.elicit::<ApprovalForm>(message).await {
        Ok(Some(form)) if form.approve => Native::Approved,
        Ok(Some(_)) => Native::Denied("refusé par l'utilisateur".into()),
        Ok(None) => Native::Denied("aucune réponse".into()),
        Err(ElicitationError::CapabilityNotSupported) => Native::Unsupported,
        Err(ElicitationError::UserDeclined) => Native::Denied("refusé par l'utilisateur".into()),
        Err(e) => Native::Denied(format!("erreur d'élicitation: {e}")),
    }
}
