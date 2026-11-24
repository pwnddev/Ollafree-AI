use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const DEFAULT_BILLION_CONTEXT_ENDPOINT: &str = "http://127.0.0.1:18787/v1/chat/completions";

pub fn default_bc_endpoint() -> String {
    DEFAULT_BILLION_CONTEXT_ENDPOINT.to_string()
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ModelEntry {
    pub endpoint: String,
    pub remote_model: String,
    pub tokens: Vec<String>,
    #[serde(default)]
    pub current_token_idx: usize,
    #[serde(default)]
    pub billion_context_enabled: bool,
    #[serde(default = "default_bc_endpoint")]
    pub billion_context_endpoint: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct Store {
    pub models: HashMap<String, ModelEntry>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
}

#[derive(Deserialize)]
pub struct ChatChoice {
    pub message: AssistantMessage,
}

#[derive(Deserialize)]
pub struct AssistantMessage {
    pub content: String,
}

#[derive(Deserialize)]
pub struct ChatResponse {
    pub choices: Vec<ChatChoice>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FreeModelPreset {
    pub alias: String,
    pub provider: String,
    pub endpoint: String,
    pub remote_model: String,
    pub notes: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CommunityPlugin {
    pub id: String,
    pub name: String,
    pub repo_slug: String,
    pub repo_url: String,
    pub description: String,
    pub proxy_endpoint: String,
    pub start_cmd: String,
    pub verified: bool,
    pub custom: bool,
    #[serde(default)]
    pub stars: Option<u64>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct PluginsStore {
    pub plugins: Vec<CommunityPlugin>,
    pub installed_ids: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct InstalledSkill {
    pub name: String,
    pub source: String,
    pub local_path: String,
    pub summary: String,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct SkillsStore {
    pub skills: Vec<InstalledSkill>,
}

#[derive(Deserialize)]
pub struct GitHubRepoInfo {
    pub full_name: String,
    pub html_url: String,
    pub description: Option<String>,
    pub stargazers_count: Option<u64>,
}

#[derive(Deserialize)]
pub struct OpenRouterModelsResponse {
    pub data: Vec<OpenRouterModelItem>,
}

#[derive(Deserialize)]
pub struct OpenRouterModelItem {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub context_length: Option<u64>,
    #[serde(default)]
    pub pricing: Option<OpenRouterPricing>,
}

#[derive(Deserialize)]
pub struct OpenRouterPricing {
    #[serde(default)]
    pub prompt: Option<serde_json::Value>,
    #[serde(default)]
    pub completion: Option<serde_json::Value>,
}

pub enum WorkerEvent {
    ChatSuccess {
        alias: String,
        reply: String,
        next_token_idx: usize,
    },
    ChatError {
        alias: String,
        error: String,
    },
    CatalogUpdated {
        presets: Vec<FreeModelPreset>,
        source_label: String,
    },
    CatalogError {
        error: String,
    },
    PluginsRefreshed {
        plugins: Vec<CommunityPlugin>,
        updated_count: usize,
    },
    PluginsRefreshError {
        error: String,
    },
    InstallFinished {
        target_label: String,
        ok: bool,
        message: String,
    },
}

#[derive(PartialEq, Eq)]
pub enum UiTab {
    Chat,
    Catalogo,
    Tokens,
    PluginsSkills,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TokenTargetScope {
    ModeloActivo,
    EndpointCoincidente,
    TodosLosModelos,
}
