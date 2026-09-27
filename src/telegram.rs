//! Canal de confirmation externe (Telegram) pour le mode headless.
//!
//! `notify_pending` envoie un message avec deux boutons inline
//! (callback_data = "approve:<id>" / "deny:<id>"). Le webhook Telegram doit
//! pointer vers `POST /telegram/webhook` (voir `src/main.rs`), qui appelle
//! `ConfirmationStore::resolve` avec la décision correspondante.
//!
//! ⚠️ Nécessite que le bot ait `setWebhook` configuré vers une URL publique
//! HTTPS de ton VPS — ce n'est pas fait automatiquement ici (étape manuelle
//! côté Telegram, une seule fois).

use crate::config::TelegramConfig;
use anyhow::{Context, Result};
use serde_json::json;

pub async fn notify_pending(cfg: &TelegramConfig, confirmation_id: &str, description: &str) -> Result<()> {
    if !cfg.enabled {
        return Ok(());
    }
    let url = format!("https://api.telegram.org/bot{}/sendMessage", cfg.bot_token);
    let body = json!({
        "chat_id": cfg.chat_id,
        "text": format!("⚠️ Action en attente d'approbation:\n\n{description}"),
        "reply_markup": {
            "inline_keyboard": [[
                { "text": "✅ Approuver", "callback_data": format!("approve:{confirmation_id}") },
                { "text": "❌ Refuser",   "callback_data": format!("deny:{confirmation_id}") },
            ]]
        }
    });

    let client = reqwest::Client::new();
    client
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("échec de l'envoi de la notification Telegram")?;
    Ok(())
}

/// Payload minimal d'un webhook `callback_query` de Telegram.
/// Le format complet est plus riche — on n'extrait que ce dont on a besoin.
#[derive(Debug, serde::Deserialize)]
pub struct TelegramWebhookPayload {
    pub callback_query: Option<CallbackQuery>,
}

#[derive(Debug, serde::Deserialize)]
pub struct CallbackQuery {
    pub data: String,
}

/// Parse "approve:<id>" / "deny:<id>" -> (id, décision approuvée?)
pub fn parse_callback_data(data: &str) -> Option<(&str, bool)> {
    if let Some(id) = data.strip_prefix("approve:") {
        Some((id, true))
    } else if let Some(id) = data.strip_prefix("deny:") {
        Some((id, false))
    } else {
        None
    }
}
