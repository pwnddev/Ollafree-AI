use crate::models::{CommunityPlugin, GitHubRepoInfo, DEFAULT_BILLION_CONTEXT_ENDPOINT};
use anyhow::{Context, Result};
use reqwest::Client;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

pub fn default_verified_plugins() -> Vec<CommunityPlugin> {
    vec![
        CommunityPlugin {
            id: "billion-context".to_string(),
            name: "billion-context".to_string(),
            repo_slug: "ranxianglei/billion-context".to_string(),
            repo_url: "https://github.com/ranxianglei/billion-context".to_string(),
            description: "Compresión de contexto en capas para sesiones largas y ventanas compactas.".to_string(),
            proxy_endpoint: DEFAULT_BILLION_CONTEXT_ENDPOINT.to_string(),
            start_cmd: "bili start".to_string(),
            verified: true,
            custom: false,
            stars: None,
        },
        CommunityPlugin {
            id: "billion-context-pi".to_string(),
            name: "billion-context-pi".to_string(),
            repo_slug: "ranxianglei/billion-context-pi".to_string(),
            repo_url: "https://github.com/ranxianglei/billion-context-pi".to_string(),
            description: "Adaptador de poda activa de contexto (ACP) integrado con billion-context.".to_string(),
            proxy_endpoint: DEFAULT_BILLION_CONTEXT_ENDPOINT.to_string(),
            start_cmd: "".to_string(),
            verified: true,
            custom: false,
            stars: None,
        },
    ]
}

pub fn parse_tokens_flexible(raw: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for part in raw.split(|c: char| c == ',' || c == '\n' || c == '\r' || c.is_whitespace()) {
        let cleaned = part.trim().trim_matches('"').trim_matches('\'').to_string();
        if !cleaned.is_empty() && seen.insert(cleaned.clone()) {
            out.push(cleaned);
        }
    }
    out
}

pub fn open_url_in_browser(url: &str) -> Result<()> {
    Command::new("xdg-open")
        .arg(url)
        .spawn()
        .context("No se pudo ejecutar xdg-open")?;
    Ok(())
}

pub fn git_clone_or_pull(repo_url: &str, dest_dir: &PathBuf) -> Result<String> {
    if dest_dir.join(".git").exists() {
        let out = Command::new("git")
            .args(["-C", dest_dir.to_str().unwrap_or(""), "pull", "--ff-only"])
            .output()
            .context("No se pudo ejecutar git pull")?;
        if out.status.success() {
            return Ok(format!("Actualizado en {}", dest_dir.display()));
        }
        anyhow::bail!(
            "git pull falló: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }

    if let Some(parent) = dest_dir.parent() {
        fs::create_dir_all(parent)?;
    }

    let out = Command::new("git")
        .args(["clone", "--depth", "1", repo_url, dest_dir.to_str().unwrap_or("")])
        .output()
        .context("No se pudo ejecutar git clone (comprueba que git esté instalado)")?;

    if out.status.success() {
        Ok(format!("Instalado en {}", dest_dir.display()))
    } else {
        anyhow::bail!(
            "git clone falló: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
}

pub async fn refresh_plugins_from_github(
    client: &Client,
    current: &[CommunityPlugin],
) -> Result<(Vec<CommunityPlugin>, usize)> {
    let mut updated = current.to_vec();
    let mut refreshed_count = 0;

    for plugin in updated.iter_mut() {
        let slug = plugin.repo_slug.trim();
        if slug.is_empty() || !slug.contains('/') {
            continue;
        }
        let api_url = format!("https://api.github.com/repos/{}", slug);
        let resp = client
            .get(&api_url)
            .header("User-Agent", "ollafree-gui")
            .send()
            .await;

        if let Ok(r) = resp {
            if r.status().is_success() {
                if let Ok(info) = r.json::<GitHubRepoInfo>().await {
                    plugin.repo_slug = info.full_name;
                    plugin.repo_url = info.html_url;
                    plugin.stars = info.stargazers_count;
                    if let Some(desc) = info.description {
                        if !desc.trim().is_empty() {
                            plugin.description = desc;
                        }
                    }
                    refreshed_count += 1;
                }
            }
        }
    }

    if refreshed_count == 0 {
        anyhow::bail!("No se pudo refrescar ningún repositorio desde GitHub.");
    }

    Ok((updated, refreshed_count))
}
