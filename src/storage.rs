use crate::models::{FreeModelPreset, PluginsStore, SkillsStore, Store};
use crate::plugins::default_verified_plugins;
use anyhow::{Context, Result};
use directories::ProjectDirs;
use std::fs;
use std::path::PathBuf;

pub fn default_presets() -> Vec<FreeModelPreset> {
    const OPENROUTER: &str = "https://openrouter.ai/api/v1/chat/completions";
    const GROQ: &str = "https://api.groq.com/openai/v1/chat/completions";

    let raw = [
        ("llama-3.3-70b-free", "OpenRouter", OPENROUTER, "meta-llama/llama-3.3-70b-instruct:free", "Uso general y razonamiento"),
        ("qwen-2.5-coder-32b-free", "OpenRouter", OPENROUTER, "qwen/qwen-2.5-coder-32b-instruct:free", "Código, bash y depuración"),
        ("qwen-2.5-72b-free", "OpenRouter", OPENROUTER, "qwen/qwen-2.5-72b-instruct:free", "Análisis y redacción amplia"),
        ("deepseek-r1-free", "OpenRouter", OPENROUTER, "deepseek/deepseek-r1:free", "Razonamiento paso a paso"),
        ("deepseek-chat-v3-free", "OpenRouter", OPENROUTER, "deepseek/deepseek-chat-v3-0324:free", "Chat general y programación"),
        ("mistral-small-24b-free", "OpenRouter", OPENROUTER, "mistralai/mistral-small-24b-instruct-2501:free", "Conversación y síntesis"),
        ("gemma-3-27b-free", "OpenRouter", OPENROUTER, "google/gemma-3-27b-it:free", "Texto y multimodal"),
        ("gemma-3-12b-free", "OpenRouter", OPENROUTER, "google/gemma-3-12b-it:free", "Equilibrio velocidad/calidad"),
        ("groq-llama-3.3-70b", "Groq", GROQ, "llama-3.3-70b-versatile", "Alta velocidad (requiere token Groq)"),
        ("groq-llama-3.1-8b-instant", "Groq", GROQ, "llama-3.1-8b-instant", "Latencia mínima (requiere token Groq)"),
        ("groq-qwen-qwq-32b", "Groq", GROQ, "qwen-qwq-32b", "Razonamiento en Groq"),
    ];

    raw.into_iter()
        .map(|(alias, provider, endpoint, remote_model, notes)| FreeModelPreset {
            alias: alias.to_string(),
            provider: provider.to_string(),
            endpoint: endpoint.to_string(),
            remote_model: remote_model.to_string(),
            notes: notes.to_string(),
        })
        .collect()
}

pub fn app_config_dir() -> Result<PathBuf> {
    let proj_dirs = ProjectDirs::from("local", "follama", "free-ollama")
        .context("No se pudo determinar el directorio de configuración local")?;
    let dir = proj_dirs.config_dir().to_path_buf();
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn plugins_install_dir() -> Result<PathBuf> {
    let dir = app_config_dir()?.join("plugins");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn skills_install_dir() -> Result<PathBuf> {
    let dir = app_config_dir()?.join("skills");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn config_path() -> Result<PathBuf> {
    Ok(app_config_dir()?.join("models.json"))
}

fn catalog_cache_path() -> Result<PathBuf> {
    Ok(app_config_dir()?.join("catalog_cache.json"))
}

fn plugins_store_path() -> Result<PathBuf> {
    Ok(app_config_dir()?.join("plugins.json"))
}

fn skills_store_path() -> Result<PathBuf> {
    Ok(app_config_dir()?.join("skills.json"))
}

pub fn load_store() -> Result<Store> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(Store::default());
    }
    let raw = fs::read_to_string(&path)?;
    let store: Store = serde_json::from_str(&raw).unwrap_or_default();
    Ok(store)
}

pub fn save_store(store: &Store) -> Result<()> {
    let path = config_path()?;
    let json = serde_json::to_string_pretty(store)?;
    fs::write(path, json)?;
    Ok(())
}

pub fn load_plugins_store() -> PluginsStore {
    let mut store = plugins_store_path()
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|raw| serde_json::from_str::<PluginsStore>(&raw).ok())
        .unwrap_or_default();

    if store.plugins.is_empty() {
        store.plugins = default_verified_plugins();
    } else {
        for base in default_verified_plugins() {
            if !store.plugins.iter().any(|p| p.id == base.id) {
                store.plugins.push(base);
            }
        }
    }
    store
}

pub fn save_plugins_store(store: &PluginsStore) -> Result<()> {
    let path = plugins_store_path()?;
    let json = serde_json::to_string_pretty(store)?;
    fs::write(path, json)?;
    Ok(())
}

pub fn load_skills_store() -> SkillsStore {
    skills_store_path()
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|raw| serde_json::from_str::<SkillsStore>(&raw).ok())
        .unwrap_or_default()
}

pub fn save_skills_store(store: &SkillsStore) -> Result<()> {
    let path = skills_store_path()?;
    let json = serde_json::to_string_pretty(store)?;
    fs::write(path, json)?;
    Ok(())
}

pub fn load_catalog() -> Vec<FreeModelPreset> {
    if let Ok(path) = catalog_cache_path() {
        if path.exists() {
            if let Ok(raw) = fs::read_to_string(&path) {
                if let Ok(list) = serde_json::from_str::<Vec<FreeModelPreset>>(&raw) {
                    if !list.is_empty() {
                        return list;
                    }
                }
            }
        }
    }
    default_presets()
}

pub fn save_catalog(list: &[FreeModelPreset]) -> Result<()> {
    let path = catalog_cache_path()?;
    let json = serde_json::to_string_pretty(list)?;
    fs::write(path, json)?;
    Ok(())
}
