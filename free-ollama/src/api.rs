use crate::models::{
    ChatMessage, ChatRequest, ChatResponse, FreeModelPreset, ModelEntry,
    OpenRouterModelsResponse, DEFAULT_BILLION_CONTEXT_ENDPOINT,
};
use crate::storage::default_presets;
use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::Value;

fn make_alias_from_id(id: &str) -> String {
    let base = id.split('/').last().unwrap_or(id);
    let clean = base
        .replace(":free", "-free")
        .replace("-instruct", "")
        .replace('_', "-");
    clean.to_lowercase()
}

fn json_val_is_zero(val: &Option<Value>) -> bool {
    match val {
        Some(Value::Number(n)) => n.as_f64().map(|x| x == 0.0).unwrap_or(false),
        Some(Value::String(s)) => s.trim().parse::<f64>().map(|x| x == 0.0).unwrap_or(false),
        _ => false,
    }
}

pub async fn fetch_live_free_catalog(client: &Client) -> Result<Vec<FreeModelPreset>> {
    let resp = client
        .get("https://openrouter.ai/api/v1/models")
        .send()
        .await
        .context("No se pudo conectar con OpenRouter")?;

    if !resp.status().is_success() {
        anyhow::bail!("OpenRouter devolvió HTTP {}", resp.status());
    }

    let parsed: OpenRouterModelsResponse = resp
        .json()
        .await
        .context("Error leyendo la lista JSON de OpenRouter")?;

    let mut result: Vec<FreeModelPreset> = Vec::new();

    for item in parsed.data {
        let free_by_suffix = item.id.ends_with(":free");
        let free_by_price = item
            .pricing
            .as_ref()
            .map(|p| json_val_is_zero(&p.prompt) && json_val_is_zero(&p.completion))
            .unwrap_or(false);

        if free_by_suffix || free_by_price {
            let ctx_note = match item.context_length {
                Some(ctx) if ctx > 0 => format!("{} • Contexto: {} tokens", item.name, ctx),
                _ => item.name.clone(),
            };

            result.push(FreeModelPreset {
                alias: make_alias_from_id(&item.id),
                provider: "OpenRouter".to_string(),
                endpoint: "https://openrouter.ai/api/v1/chat/completions".to_string(),
                remote_model: item.id,
                notes: if ctx_note.trim().is_empty() {
                    "Modelo gratuito sincronizado desde OpenRouter".to_string()
                } else {
                    ctx_note
                },
            });
        }
    }

    for preset in default_presets() {
        if preset.provider == "Groq" {
            result.push(preset);
        }
    }

    result.sort_by(|a, b| a.alias.cmp(&b.alias));
    result.dedup_by(|a, b| a.remote_model == b.remote_model && a.endpoint == b.endpoint);

    if result.is_empty() {
        anyhow::bail!("No se encontraron modelos gratuitos en la respuesta.");
    }

    Ok(result)
}

pub async fn fetch_custom_json_catalog(client: &Client, url: &str) -> Result<Vec<FreeModelPreset>> {
    let resp = client
        .get(url)
        .send()
        .await
        .context("No se pudo descargar la lista desde la URL")?;

    if !resp.status().is_success() {
        anyhow::bail!("La URL devolvió HTTP {}", resp.status());
    }

    let list: Vec<FreeModelPreset> = resp
        .json()
        .await
        .context("El JSON remoto no tiene el formato esperado")?;

    if list.is_empty() {
        anyhow::bail!("La lista descargada está vacía.");
    }

    Ok(list)
}

pub fn effective_endpoint(entry: &ModelEntry) -> String {
    if entry.billion_context_enabled {
        let bc = entry.billion_context_endpoint.trim();
        if !bc.is_empty() {
            return bc.to_string();
        }
        return DEFAULT_BILLION_CONTEXT_ENDPOINT.to_string();
    }
    entry.endpoint.clone()
}

pub async fn send_with_rotation(
    client: &Client,
    entry: &mut ModelEntry,
    history: &[ChatMessage],
) -> Result<String> {
    if entry.tokens.is_empty() {
        anyhow::bail!("Este modelo no tiene tokens configurados.");
    }

    let target_url = effective_endpoint(entry);
    let total = entry.tokens.len();

    for attempt in 0..total {
        let idx = (entry.current_token_idx + attempt) % total;
        let token = &entry.tokens[idx];

        let payload = ChatRequest {
            model: entry.remote_model.clone(),
            messages: history.to_vec(),
            stream: false,
        };

        let resp = client
            .post(&target_url)
            .bearer_auth(token)
            .json(&payload)
            .send()
            .await;

        match resp {
            Ok(r) if r.status().is_success() => {
                entry.current_token_idx = (idx + 1) % total;
                let data: ChatResponse = r
                    .json()
                    .await
                    .context("Error parseando JSON de respuesta")?;
                let reply = data
                    .choices
                    .first()
                    .map(|c| c.message.content.clone())
                    .unwrap_or_default();
                return Ok(reply);
            }
            Ok(r)
                if r.status().as_u16() == 429
                    || r.status().as_u16() == 401
                    || r.status().as_u16() == 403 =>
            {
                continue;
            }
            Ok(r) => {
                let status = r.status();
                let body = r.text().await.unwrap_or_default();
                anyhow::bail!("Error HTTP {}: {}", status, body);
            }
            Err(e) => {
                if entry.billion_context_enabled {
                    anyhow::bail!(
                        "Error conectando con plugin/proxy ({}): {}. Comprueba que el servicio esté activo.",
                        target_url,
                        e
                    );
                }
                if attempt + 1 == total {
                    return Err(e.into());
                }
            }
        }
    }

    anyhow::bail!("Todos los tokens configurados fallaron o alcanzaron su límite.")
}
