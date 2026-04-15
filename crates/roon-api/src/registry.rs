use std::collections::HashMap;
use std::sync::Arc;

use roon_moo::connection::{MooConnection, ServiceHandler};

use crate::core::{Core, CoreInner};
use crate::error::ApiError;
use crate::token::StateStore;

/// Extension registration info sent to Roon Core.
#[derive(Debug, Clone)]
pub(crate) struct ExtensionInfo {
    pub extension_id: String,
    pub display_name: String,
    pub display_version: String,
    pub publisher: String,
    pub email: String,
    pub website: Option<String>,
    pub required_services: Vec<String>,
    pub optional_services: Vec<String>,
}

/// Perform the registry handshake on a MOO connection.
///
/// 1. Send `com.roonlabs.registry:1/info` → get core_id, display_name
/// 2. Look up persisted token for this core_id
/// 3. Send `com.roonlabs.registry:1/register` → get Registered response with token
/// 4. Persist the new token
///
/// Returns a `Core` handle on success.
pub(crate) async fn perform_handshake(
    url: &str,
    info: &ExtensionInfo,
    token_store: &dyn StateStore,
) -> Result<Core, ApiError> {
    // Build service handlers for provided services (ping + pairing)
    let service_handlers = build_service_handlers();

    let connection = MooConnection::connect(url, service_handlers).await?;
    let connection = Arc::new(connection);

    // Step 1: Info request
    let info_response = connection
        .send_request("com.roonlabs.registry:1/info", None)
        .await?;

    let info_body = info_response
        .json_body()
        .ok_or_else(|| ApiError::RegistryFailed("info response has no body".into()))?;

    let core_id = info_body["core_id"]
        .as_str()
        .ok_or_else(|| ApiError::RegistryFailed("missing core_id in info response".into()))?
        .to_string();
    let core_display_name = info_body["display_name"]
        .as_str()
        .unwrap_or("Unknown Core")
        .to_string();
    let core_display_version = info_body["display_version"]
        .as_str()
        .unwrap_or("")
        .to_string();

    // Step 2: Look up persisted token
    let existing_token = token_store.load_token(&core_id);

    // Step 3: Register
    let mut reg_body = serde_json::json!({
        "extension_id": info.extension_id,
        "display_name": info.display_name,
        "display_version": info.display_version,
        "publisher": info.publisher,
        "email": info.email,
        "required_services": info.required_services,
        "optional_services": info.optional_services,
        "provided_services": [
            "com.roonlabs.pairing:1",
            "com.roonlabs.ping:1"
        ]
    });

    if let Some(ref token) = existing_token {
        reg_body["token"] = serde_json::Value::String(token.clone());
    }
    if let Some(ref website) = info.website {
        reg_body["website"] = serde_json::Value::String(website.clone());
    }

    let reg_response = connection
        .send_request("com.roonlabs.registry:1/register", Some(reg_body))
        .await?;

    // The response should be CONTINUE Registered (stays open for connection lifetime)
    if reg_response.name != "Registered" {
        return Err(ApiError::RegistryFailed(format!(
            "expected 'Registered', got '{}'",
            reg_response.name
        )));
    }

    let reg_body = reg_response
        .json_body()
        .ok_or_else(|| ApiError::RegistryFailed("register response has no body".into()))?;

    let new_token = reg_body["token"].as_str().map(|s| s.to_string());
    let http_port = reg_body["http_port"].as_u64().unwrap_or(0) as u16;

    // Step 4: Persist token
    if let Some(ref token) = new_token {
        if let Err(e) = token_store.save_token(&core_id, token) {
            tracing::warn!("Failed to persist token for core {}: {}", core_id, e);
        }
    }

    Ok(Core {
        inner: Arc::new(CoreInner {
            core_id,
            display_name: core_display_name,
            display_version: core_display_version,
            token: new_token,
            http_port,
            connection,
        }),
    })
}

/// Build service handlers for the provided services (ping and pairing).
fn build_service_handlers() -> HashMap<String, ServiceHandler> {
    let mut handlers: HashMap<String, ServiceHandler> = HashMap::new();

    // Ping service: respond with COMPLETE Success
    let ping_handler: ServiceHandler = Arc::new(|_msg, responder| {
        tokio::spawn(async move {
            let _ = responder.send_complete("Success", None).await;
        });
    });
    handlers.insert("com.roonlabs.ping:1".to_string(), ping_handler);

    // Pairing service: basic implementation
    let pairing_handler: ServiceHandler = Arc::new(|msg, responder| {
        let method = msg.method().unwrap_or("").to_string();
        tokio::spawn(async move {
            match method.as_str() {
                "get_pairing" => {
                    let _ = responder
                        .send_complete("Success", Some(serde_json::json!({})))
                        .await;
                }
                "pair" => {
                    let _ = responder.send_complete("Success", None).await;
                }
                "subscribe_pairing" => {
                    let _ = responder
                        .send_continue("Subscribed", Some(serde_json::json!({})))
                        .await;
                }
                _ => {
                    let _ = responder
                        .send_complete(
                            "InvalidRequest",
                            Some(serde_json::json!({"error": format!("unknown method: {}", method)})),
                        )
                        .await;
                }
            }
        });
    });
    handlers.insert("com.roonlabs.pairing:1".to_string(), pairing_handler);

    handlers
}
