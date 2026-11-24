use crate::api::{
    effective_endpoint, fetch_custom_json_catalog, fetch_live_free_catalog, send_with_rotation,
};
use crate::models::{
    ChatMessage, CommunityPlugin, FreeModelPreset, InstalledSkill, ModelEntry, PluginsStore,
    SkillsStore, Store, TokenTargetScope, UiTab, WorkerEvent, DEFAULT_BILLION_CONTEXT_ENDPOINT,
};
use crate::plugins::{
    git_clone_or_pull, open_url_in_browser, parse_tokens_flexible, refresh_plugins_from_github,
};
use crate::storage::{
    default_presets, load_catalog, load_plugins_store, load_skills_store, load_store,
    plugins_install_dir, save_catalog, save_plugins_store, save_skills_store, save_store,
    skills_install_dir,
};
use eframe::egui;
use reqwest::Client;
use std::collections::HashMap;
use std::fs;
use std::sync::mpsc::{self, Receiver, Sender};

pub struct OllaFreeGuiApp {
    store: Store,
    selected_alias: Option<String>,
    histories: HashMap<String, Vec<ChatMessage>>,
    active_tab: UiTab,

    presets: Vec<FreeModelPreset>,
    catalog_search: String,
    catalog_provider_filter: String,
    default_import_tokens: String,
    custom_import_alias: String,
    custom_catalog_url: String,
    is_updating_catalog: bool,
    import_with_billion_context: bool,
    default_bc_url: String,

    token_bulk_input: String,
    token_scope: TokenTargetScope,
    token_endpoint_filter: String,

    plugins_store: PluginsStore,
    skills_store: SkillsStore,
    is_refreshing_plugins: bool,
    custom_plugin_name: String,
    custom_plugin_repo: String,
    custom_plugin_proxy: String,
    custom_plugin_cmd: String,
    custom_plugin_desc: String,
    skill_name_input: String,
    skill_repo_or_md_input: String,
    skill_summary_input: String,

    new_alias: String,
    new_endpoint: String,
    new_model: String,
    new_tokens: String,
    new_bc_enabled: bool,

    extra_tokens: String,

    prompt_input: String,
    is_loading: bool,
    status_msg: String,

    rt: tokio::runtime::Runtime,
    tx: Sender<WorkerEvent>,
    rx: Receiver<WorkerEvent>,
    http_client: Client,
}

impl OllaFreeGuiApp {
    pub fn new() -> Self {
        let store = load_store().unwrap_or_default();
        let selected_alias = store.models.keys().next().cloned();
        let mut presets = load_catalog();
        if presets.is_empty() {
            presets = default_presets();
        }
        let plugins_store = load_plugins_store();
        let skills_store = load_skills_store();
        let rt = tokio::runtime::Runtime::new().expect("No se pudo iniciar Tokio runtime");
        let (tx, rx) = mpsc::channel();

        Self {
            store,
            selected_alias,
            histories: HashMap::new(),
            active_tab: UiTab::Catalogo,
            presets,
            catalog_search: String::new(),
            catalog_provider_filter: "Todos".to_string(),
            default_import_tokens: String::new(),
            custom_import_alias: String::new(),
            custom_catalog_url: String::new(),
            is_updating_catalog: false,
            import_with_billion_context: false,
            default_bc_url: DEFAULT_BILLION_CONTEXT_ENDPOINT.to_string(),
            token_bulk_input: String::new(),
            token_scope: TokenTargetScope::TodosLosModelos,
            token_endpoint_filter: "https://openrouter.ai/api/v1/chat/completions".to_string(),
            plugins_store,
            skills_store,
            is_refreshing_plugins: false,
            custom_plugin_name: String::new(),
            custom_plugin_repo: String::new(),
            custom_plugin_proxy: DEFAULT_BILLION_CONTEXT_ENDPOINT.to_string(),
            custom_plugin_cmd: String::new(),
            custom_plugin_desc: String::new(),
            skill_name_input: String::new(),
            skill_repo_or_md_input: String::new(),
            skill_summary_input: String::new(),
            new_alias: String::new(),
            new_endpoint: "https://openrouter.ai/api/v1/chat/completions".to_string(),
            new_model: String::new(),
            new_tokens: String::new(),
            new_bc_enabled: false,
            extra_tokens: String::new(),
            prompt_input: String::new(),
            is_loading: false,
            status_msg: "Listo: 0 complicaciones.".to_string(),
            rt,
            tx,
            rx,
            http_client: Client::new(),
        }
    }

    fn persist(&mut self) {
        if let Err(e) = save_store(&self.store) {
            self.status_msg = format!("Error guardando config: {}", e);
        }
    }

    fn persist_plugins(&mut self) {
        if let Err(e) = save_plugins_store(&self.plugins_store) {
            self.status_msg = format!("Error guardando plugins: {}", e);
        }
    }

    fn persist_skills(&mut self) {
        if let Err(e) = save_skills_store(&self.skills_store) {
            self.status_msg = format!("Error guardando skills: {}", e);
        }
    }

    fn unique_alias(&self, base: &str) -> String {
        if !self.store.models.contains_key(base) {
            return base.to_string();
        }
        let mut n = 2;
        loop {
            let candidate = format!("{}-{}", base, n);
            if !self.store.models.contains_key(&candidate) {
                return candidate;
            }
            n += 1;
        }
    }

    fn trigger_refresh_plugins(&mut self) {
        if self.is_refreshing_plugins {
            return;
        }
        self.is_refreshing_plugins = true;
        self.status_msg = "Actualizando metadatos de plugins desde GitHub...".to_string();

        let client = self.http_client.clone();
        let current = self.plugins_store.plugins.clone();
        let tx = self.tx.clone();

        self.rt.spawn(async move {
            match refresh_plugins_from_github(&client, &current).await {
                Ok((plugins, updated_count)) => {
                    let _ = tx.send(WorkerEvent::PluginsRefreshed {
                        plugins,
                        updated_count,
                    });
                }
                Err(e) => {
                    let _ = tx.send(WorkerEvent::PluginsRefreshError {
                        error: e.to_string(),
                    });
                }
            }
        });
    }

    fn install_plugin_repo(&mut self, plugin: &CommunityPlugin) {
        let id = plugin.id.clone();
        let name = plugin.name.clone();
        let repo_url = plugin.repo_url.clone();
        let tx = self.tx.clone();

        if !self.plugins_store.installed_ids.contains(&id) {
            self.plugins_store.installed_ids.push(id.clone());
            self.persist_plugins();
        }

        self.status_msg = format!("Instalando plugin '{}'...", name);
        std::thread::spawn(move || {
            let res =
                plugins_install_dir().and_then(|base| git_clone_or_pull(&repo_url, &base.join(&id)));
            match res {
                Ok(msg) => {
                    let _ = tx.send(WorkerEvent::InstallFinished {
                        target_label: name,
                        ok: true,
                        message: msg,
                    });
                }
                Err(e) => {
                    let _ = tx.send(WorkerEvent::InstallFinished {
                        target_label: name,
                        ok: false,
                        message: e.to_string(),
                    });
                }
            }
        });
    }

    fn add_custom_plugin(&mut self) {
        let name = self.custom_plugin_name.trim().to_string();
        if name.is_empty() {
            self.status_msg = "Ponle un nombre a tu plugin personalizado.".to_string();
            return;
        }
        let id = name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .trim_matches('-')
            .to_string();
        let id = if id.is_empty() {
            "custom-plugin".to_string()
        } else {
            id
        };

        let raw_repo = self.custom_plugin_repo.trim().to_string();
        let (repo_slug, repo_url) =
            if raw_repo.starts_with("http://") || raw_repo.starts_with("https://") {
                let slug = raw_repo
                    .trim_end_matches('/')
                    .split("github.com/")
                    .nth(1)
                    .unwrap_or("")
                    .trim_end_matches(".git")
                    .to_string();
                (slug, raw_repo)
            } else if raw_repo.contains('/') {
                let slug = raw_repo.trim_end_matches(".git").to_string();
                let url = format!("https://github.com/{}", slug);
                (slug, url)
            } else {
                (String::new(), raw_repo)
            };

        let proxy = if self.custom_plugin_proxy.trim().is_empty() {
            DEFAULT_BILLION_CONTEXT_ENDPOINT.to_string()
        } else {
            self.custom_plugin_proxy.trim().to_string()
        };

        let desc = if self.custom_plugin_desc.trim().is_empty() {
            "Plugin personalizado añadido por el usuario".to_string()
        } else {
            self.custom_plugin_desc.trim().to_string()
        };

        let plugin = CommunityPlugin {
            id: id.clone(),
            name: name.clone(),
            repo_slug,
            repo_url: repo_url.clone(),
            description: desc,
            proxy_endpoint: proxy,
            start_cmd: self.custom_plugin_cmd.trim().to_string(),
            verified: false,
            custom: true,
            stars: None,
        };

        self.plugins_store.plugins.retain(|p| p.id != id);
        self.plugins_store.plugins.push(plugin.clone());
        if !self.plugins_store.installed_ids.contains(&id) {
            self.plugins_store.installed_ids.push(id);
        }
        self.persist_plugins();

        self.custom_plugin_name.clear();
        self.custom_plugin_repo.clear();
        self.custom_plugin_cmd.clear();
        self.custom_plugin_desc.clear();

        if repo_url.starts_with("http://") || repo_url.starts_with("https://") {
            self.install_plugin_repo(&plugin);
        } else {
            self.status_msg = format!("Plugin propio '{}' guardado.", name);
        }
    }

    fn activate_plugin_on_selected_model(&mut self, plugin: &CommunityPlugin) {
        let Some(alias) = self.selected_alias.clone() else {
            self.status_msg = "Selecciona un modelo primero para activar el plugin.".to_string();
            return;
        };
        if let Some(entry) = self.store.models.get_mut(&alias) {
            entry.billion_context_enabled = true;
            entry.billion_context_endpoint = plugin.proxy_endpoint.clone();
        }
        self.persist();
        self.status_msg = format!(
            "Plugin '{}' activado en '{}' ({})",
            plugin.name, alias, plugin.proxy_endpoint
        );
    }

    fn install_skill(&mut self) {
        let raw_name = self.skill_name_input.trim().to_string();
        let source_or_md = self.skill_repo_or_md_input.trim().to_string();
        let summary_input = self.skill_summary_input.trim().to_string();

        if raw_name.is_empty() || source_or_md.is_empty() {
            self.status_msg =
                "Indica el nombre de la skill y una URL/repo de GitHub o el contenido.".to_string();
            return;
        }

        let safe_name: String = raw_name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .trim_matches('-')
            .to_string();
        let folder_name = if safe_name.is_empty() {
            "custom-skill".to_string()
        } else {
            safe_name
        };

        let Ok(skills_root) = skills_install_dir() else {
            self.status_msg = "No se pudo crear el directorio local de skills.".to_string();
            return;
        };
        let dest = skills_root.join(&folder_name);

        let is_git_url = source_or_md.starts_with("http://")
            || source_or_md.starts_with("https://")
            || source_or_md.ends_with(".git");

        if is_git_url {
            let tx = self.tx.clone();
            let skill_label = raw_name.clone();
            let src_clone = source_or_md.clone();
            let dest_clone = dest.clone();
            let sum_clone = if summary_input.is_empty() {
                format!("Skill clonada desde {}", source_or_md)
            } else {
                summary_input.clone()
            };

            self.skills_store.skills.retain(|s| s.name != raw_name);
            self.skills_store.skills.push(InstalledSkill {
                name: raw_name,
                source: source_or_md,
                local_path: dest.to_string_lossy().to_string(),
                summary: sum_clone,
            });
            self.persist_skills();

            self.skill_name_input.clear();
            self.skill_repo_or_md_input.clear();
            self.skill_summary_input.clear();

            self.status_msg = format!("Clonando skill '{}'...", skill_label);
            std::thread::spawn(move || match git_clone_or_pull(&src_clone, &dest_clone) {
                Ok(msg) => {
                    let _ = tx.send(WorkerEvent::InstallFinished {
                        target_label: skill_label,
                        ok: true,
                        message: msg,
                    });
                }
                Err(e) => {
                    let _ = tx.send(WorkerEvent::InstallFinished {
                        target_label: skill_label,
                        ok: false,
                        message: e.to_string(),
                    });
                }
            });
        } else {
            if let Err(e) = fs::create_dir_all(&dest) {
                self.status_msg = format!("Error creando carpeta de skill: {}", e);
                return;
            }
            let md_path = dest.join("SKILL.md");
            let content = if source_or_md.contains('\n') || source_or_md.starts_with('#') {
                source_or_md.clone()
            } else {
                format!("# {}\n\n{}\n", raw_name, source_or_md)
            };
            if let Err(e) = fs::write(&md_path, content) {
                self.status_msg = format!("Error escribiendo SKILL.md: {}", e);
                return;
            }

            let sum = if summary_input.is_empty() {
                "Skill local guardada en SKILL.md".to_string()
            } else {
                summary_input
            };

            self.skills_store.skills.retain(|s| s.name != raw_name);
            self.skills_store.skills.push(InstalledSkill {
                name: raw_name.clone(),
                source: "local".to_string(),
                local_path: md_path.to_string_lossy().to_string(),
                summary: sum,
            });
            self.persist_skills();

            self.skill_name_input.clear();
            self.skill_repo_or_md_input.clear();
            self.skill_summary_input.clear();
            self.status_msg = format!("Skill '{}' instalada en {}", raw_name, md_path.display());
        }
    }

    fn apply_bulk_tokens(&mut self, replace_existing: bool) {
        let incoming = parse_tokens_flexible(&self.token_bulk_input);
        if incoming.is_empty() {
            self.status_msg = "Pega al menos un token antes de importar.".to_string();
            return;
        }
        if self.store.models.is_empty() {
            self.status_msg =
                "Primero importa o crea al menos un modelo en 'Modelos'.".to_string();
            return;
        }

        let target_endpoint = self.token_endpoint_filter.trim().to_string();
        let active = self.selected_alias.clone();
        let mut updated_models = 0;

        for (alias, entry) in self.store.models.iter_mut() {
            let matches_scope = match self.token_scope {
                TokenTargetScope::TodosLosModelos => true,
                TokenTargetScope::ModeloActivo => active.as_ref() == Some(alias),
                TokenTargetScope::EndpointCoincidente => {
                    target_endpoint.is_empty() || entry.endpoint == target_endpoint
                }
            };

            if matches_scope {
                if replace_existing {
                    entry.tokens = incoming.clone();
                    entry.current_token_idx = 0;
                } else {
                    for tok in &incoming {
                        if !entry.tokens.contains(tok) {
                            entry.tokens.push(tok.clone());
                        }
                    }
                }
                updated_models += 1;
            }
        }

        if updated_models == 0 {
            self.status_msg = "Ningún modelo coincidió con el ámbito seleccionado.".to_string();
            return;
        }

        self.token_bulk_input.clear();
        self.persist();
        let mode_label = if replace_existing {
            "Reemplazados"
        } else {
            "Añadidos"
        };
        self.status_msg = format!(
            "{} {} token(s) en {} modelo(s).",
            mode_label,
            incoming.len(),
            updated_models
        );
    }

    fn open_provider_page(&mut self, name: &str, url: &str) {
        match open_url_in_browser(url) {
            Ok(()) => self.status_msg = format!("Abriendo {} en el navegador...", name),
            Err(e) => self.status_msg = format!("No se pudo abrir {}: {}", name, e),
        }
    }

    fn trigger_live_catalog_update(&mut self) {
        if self.is_updating_catalog {
            return;
        }
        self.is_updating_catalog = true;
        self.status_msg = "Consultando modelos gratuitos disponibles en OpenRouter...".to_string();

        let client = self.http_client.clone();
        let tx = self.tx.clone();

        self.rt.spawn(async move {
            match fetch_live_free_catalog(&client).await {
                Ok(presets) => {
                    let _ = tx.send(WorkerEvent::CatalogUpdated {
                        presets,
                        source_label: "OpenRouter API en vivo".to_string(),
                    });
                }
                Err(e) => {
                    let _ = tx.send(WorkerEvent::CatalogError {
                        error: e.to_string(),
                    });
                }
            }
        });
    }

    fn trigger_url_catalog_update(&mut self) {
        if self.is_updating_catalog {
            return;
        }
        let url = self.custom_catalog_url.trim().to_string();
        if url.is_empty() {
            self.status_msg = "Introduce una URL con el JSON de modelos.".to_string();
            return;
        }

        self.is_updating_catalog = true;
        self.status_msg = format!("Descargando catálogo desde {}...", url);

        let client = self.http_client.clone();
        let tx = self.tx.clone();

        self.rt.spawn(async move {
            match fetch_custom_json_catalog(&client, &url).await {
                Ok(presets) => {
                    let _ = tx.send(WorkerEvent::CatalogUpdated {
                        presets,
                        source_label: "URL personalizada".to_string(),
                    });
                }
                Err(e) => {
                    let _ = tx.send(WorkerEvent::CatalogError {
                        error: e.to_string(),
                    });
                }
            }
        });
    }

    fn import_preset(&mut self, preset: &FreeModelPreset, switch_to_chat: bool) {
        let chosen_alias = if self.custom_import_alias.trim().is_empty() {
            self.unique_alias(&preset.alias)
        } else {
            self.custom_import_alias.trim().to_string()
        };

        let shared_tokens = parse_tokens_flexible(&self.default_import_tokens);
        let initial_tokens = if shared_tokens.is_empty() {
            self.store
                .models
                .values()
                .find(|m| m.endpoint == preset.endpoint && !m.tokens.is_empty())
                .map(|m| m.tokens.clone())
                .unwrap_or_default()
        } else {
            shared_tokens
        };

        let bc_url = if self.default_bc_url.trim().is_empty() {
            DEFAULT_BILLION_CONTEXT_ENDPOINT.to_string()
        } else {
            self.default_bc_url.trim().to_string()
        };

        let token_count = initial_tokens.len();
        self.store.models.insert(
            chosen_alias.clone(),
            ModelEntry {
                endpoint: preset.endpoint.clone(),
                remote_model: preset.remote_model.clone(),
                tokens: initial_tokens,
                current_token_idx: 0,
                billion_context_enabled: self.import_with_billion_context,
                billion_context_endpoint: bc_url,
            },
        );
        self.selected_alias = Some(chosen_alias.clone());
        self.custom_import_alias.clear();
        self.persist();

        if switch_to_chat {
            self.active_tab = UiTab::Chat;
        }

        if token_count == 0 {
            self.status_msg = format!(
                "Importado '{}'. Añade tokens en '🔑 Tokens' o en el chat.",
                chosen_alias
            );
        } else {
            self.status_msg = format!(
                "Importado '{}' con {} token(s) listo para usar.",
                chosen_alias, token_count
            );
        }
    }

    fn import_all_filtered(&mut self, filtered: &[FreeModelPreset]) {
        let mut imported = 0;
        let shared_tokens = parse_tokens_flexible(&self.default_import_tokens);
        let bc_url = if self.default_bc_url.trim().is_empty() {
            DEFAULT_BILLION_CONTEXT_ENDPOINT.to_string()
        } else {
            self.default_bc_url.trim().to_string()
        };

        for preset in filtered {
            let already_exists = self
                .store
                .models
                .values()
                .any(|m| m.remote_model == preset.remote_model && m.endpoint == preset.endpoint);
            if already_exists {
                continue;
            }

            let alias = self.unique_alias(&preset.alias);
            let tokens = if !shared_tokens.is_empty() {
                shared_tokens.clone()
            } else {
                self.store
                    .models
                    .values()
                    .find(|m| m.endpoint == preset.endpoint && !m.tokens.is_empty())
                    .map(|m| m.tokens.clone())
                    .unwrap_or_default()
            };

            self.store.models.insert(
                alias.clone(),
                ModelEntry {
                    endpoint: preset.endpoint.clone(),
                    remote_model: preset.remote_model.clone(),
                    tokens,
                    current_token_idx: 0,
                    billion_context_enabled: self.import_with_billion_context,
                    billion_context_endpoint: bc_url.clone(),
                },
            );
            if self.selected_alias.is_none() {
                self.selected_alias = Some(alias);
            }
            imported += 1;
        }

        self.persist();
        self.status_msg = format!(
            "Importación masiva completada: {} modelo(s) nuevo(s) añadido(s).",
            imported
        );
    }

    fn poll_worker(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                WorkerEvent::ChatSuccess {
                    alias,
                    reply,
                    next_token_idx,
                } => {
                    self.is_loading = false;
                    if let Some(entry) = self.store.models.get_mut(&alias) {
                        entry.current_token_idx = next_token_idx;
                    }
                    self.persist();
                    self.histories
                        .entry(alias.clone())
                        .or_default()
                        .push(ChatMessage {
                            role: "assistant".to_string(),
                            content: reply,
                        });
                    self.status_msg = format!("Respuesta recibida de '{}'", alias);
                }
                WorkerEvent::ChatError { alias, error } => {
                    self.is_loading = false;
                    if let Some(hist) = self.histories.get_mut(&alias) {
                        if hist.last().map(|m| m.role.as_str()) == Some("user") {
                            hist.pop();
                        }
                    }
                    self.status_msg = format!("Error: {}", error);
                }
                WorkerEvent::CatalogUpdated {
                    presets,
                    source_label,
                } => {
                    self.is_updating_catalog = false;
                    let total = presets.len();
                    self.presets = presets;
                    if let Err(e) = save_catalog(&self.presets) {
                        self.status_msg = format!(
                            "Lista actualizada ({} modelos desde {}), pero falló la caché: {}",
                            total, source_label, e
                        );
                    } else {
                        self.status_msg = format!(
                            "Lista actualizada: {} modelos gratuitos cargados desde {}.",
                            total, source_label
                        );
                    }
                }
                WorkerEvent::CatalogError { error } => {
                    self.is_updating_catalog = false;
                    self.status_msg = format!("Error actualizando catálogo: {}", error);
                }
                WorkerEvent::PluginsRefreshed {
                    plugins,
                    updated_count,
                } => {
                    self.is_refreshing_plugins = false;
                    self.plugins_store.plugins = plugins;
                    self.persist_plugins();
                    self.status_msg = format!(
                        "Plugins actualizados desde GitHub ({} repositorio(s) sincronizado(s)).",
                        updated_count
                    );
                }
                WorkerEvent::PluginsRefreshError { error } => {
                    self.is_refreshing_plugins = false;
                    self.status_msg = format!("Error actualizando plugins: {}", error);
                }
                WorkerEvent::InstallFinished {
                    target_label,
                    ok,
                    message,
                } => {
                    if ok {
                        self.status_msg = format!("{}: {}", target_label, message);
                    } else {
                        self.status_msg = format!("Error en '{}': {}", target_label, message);
                    }
                }
            }
        }
    }

    fn send_prompt(&mut self) {
        if self.is_loading {
            return;
        }
        let text = self.prompt_input.trim().to_string();
        if text.is_empty() {
            return;
        }
        let Some(alias) = self.selected_alias.clone() else {
            self.status_msg = "Selecciona o importa un modelo primero.".to_string();
            return;
        };
        let Some(entry) = self.store.models.get(&alias).cloned() else {
            self.status_msg = "El modelo seleccionado no existe.".to_string();
            return;
        };

        let history = self.histories.entry(alias.clone()).or_default();
        history.push(ChatMessage {
            role: "user".to_string(),
            content: text,
        });
        let snapshot_history = history.clone();

        self.prompt_input.clear();
        self.is_loading = true;
        self.status_msg = if entry.billion_context_enabled {
            format!("Consultando '{}' vía plugin/proxy...", alias)
        } else {
            format!("Consultando '{}'...", alias)
        };

        let client = self.http_client.clone();
        let tx = self.tx.clone();

        self.rt.spawn(async move {
            let mut mutable_entry = entry;
            match send_with_rotation(&client, &mut mutable_entry, &snapshot_history).await {
                Ok(reply) => {
                    let _ = tx.send(WorkerEvent::ChatSuccess {
                        alias,
                        reply,
                        next_token_idx: mutable_entry.current_token_idx,
                    });
                }
                Err(e) => {
                    let _ = tx.send(WorkerEvent::ChatError {
                        alias,
                        error: e.to_string(),
                    });
                }
            }
        });
    }
}

impl eframe::App for OllaFreeGuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_worker();
        if self.is_loading || self.is_updating_catalog || self.is_refreshing_plugins {
            ctx.request_repaint();
        }

        egui::SidePanel::left("modelos_panel")
            .resizable(true)
            .default_width(315.0)
            .show(ctx, |ui| {
                ui.heading("OllaFree");
                ui.horizontal(|ui| {
                    if ui
                        .selectable_label(self.active_tab == UiTab::Chat, "💬 Chat")
                        .clicked()
                    {
                        self.active_tab = UiTab::Chat;
                    }
                    if ui
                        .selectable_label(self.active_tab == UiTab::Catalogo, "📦 Modelos")
                        .clicked()
                    {
                        self.active_tab = UiTab::Catalogo;
                    }
                    if ui
                        .selectable_label(self.active_tab == UiTab::Tokens, "🔑 Tokens")
                        .clicked()
                    {
                        self.active_tab = UiTab::Tokens;
                    }
                });
                if ui
                    .selectable_label(
                        self.active_tab == UiTab::PluginsSkills,
                        "🧩 Plugins & Skills",
                    )
                    .clicked()
                {
                    self.active_tab = UiTab::PluginsSkills;
                }

                ui.separator();
                ui.label(egui::RichText::new("Modelos instalados").strong());

                let mut aliases: Vec<String> = self.store.models.keys().cloned().collect();
                aliases.sort();

                let mut to_delete: Option<String> = None;
                let mut select_and_chat: Option<String> = None;

                egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .show(ui, |ui| {
                        if aliases.is_empty() {
                            ui.label("Sin modelos aún. Ve a 'Modelos' para importar.");
                        }
                        for alias in &aliases {
                            let is_selected = self.selected_alias.as_ref() == Some(alias);
                            let (token_count, bc_on) = self
                                .store
                                .models
                                .get(alias)
                                .map(|m| (m.tokens.len(), m.billion_context_enabled))
                                .unwrap_or((0, false));

                            let badge = if bc_on {
                                format!("{} ({} tok • 🧩BC)", alias, token_count)
                            } else {
                                format!("{} ({} tok)", alias, token_count)
                            };

                            ui.horizontal(|ui| {
                                if ui.selectable_label(is_selected, badge).clicked() {
                                    select_and_chat = Some(alias.clone());
                                }
                                if ui.small_button("🗑").clicked() {
                                    to_delete = Some(alias.clone());
                                }
                            });
                        }
                    });

                if let Some(a) = select_and_chat {
                    self.selected_alias = Some(a);
                    self.active_tab = UiTab::Chat;
                }

                if let Some(del_alias) = to_delete {
                    self.store.models.remove(&del_alias);
                    self.histories.remove(&del_alias);
                    if self.selected_alias.as_ref() == Some(&del_alias) {
                        self.selected_alias = self.store.models.keys().next().cloned();
                    }
                    self.persist();
                    self.status_msg = format!("Eliminado '{}'", del_alias);
                }

                ui.separator();
                ui.collapsing("Añadir modelo manual", |ui| {
                    ui.label("Alias local:");
                    ui.text_edit_singleline(&mut self.new_alias);

                    ui.label("Endpoint (/chat/completions):");
                    ui.text_edit_singleline(&mut self.new_endpoint);

                    ui.label("Modelo remoto:");
                    ui.text_edit_singleline(&mut self.new_model);

                    ui.label("Tokens (separados por coma):");
                    ui.text_edit_singleline(&mut self.new_tokens);

                    ui.checkbox(&mut self.new_bc_enabled, "Usar plugin billion-context");

                    if ui.button("Guardar modelo").clicked() {
                        let alias = self.new_alias.trim().to_string();
                        let endpoint = self.new_endpoint.trim().to_string();
                        let model = self.new_model.trim().to_string();
                        let tokens = parse_tokens_flexible(&self.new_tokens);

                        if alias.is_empty()
                            || endpoint.is_empty()
                            || model.is_empty()
                            || tokens.is_empty()
                        {
                            self.status_msg =
                                "Completa alias, endpoint, modelo y al menos un token.".to_string();
                        } else {
                            let bc_url = if self.default_bc_url.trim().is_empty() {
                                DEFAULT_BILLION_CONTEXT_ENDPOINT.to_string()
                            } else {
                                self.default_bc_url.trim().to_string()
                            };
                            self.store.models.insert(
                                alias.clone(),
                                ModelEntry {
                                    endpoint,
                                    remote_model: model,
                                    tokens,
                                    current_token_idx: 0,
                                    billion_context_enabled: self.new_bc_enabled,
                                    billion_context_endpoint: bc_url,
                                },
                            );
                            self.selected_alias = Some(alias.clone());
                            self.new_alias.clear();
                            self.new_model.clear();
                            self.new_tokens.clear();
                            self.new_bc_enabled = false;
                            self.persist();
                            self.active_tab = UiTab::Chat;
                            self.status_msg = format!("Modelo '{}' guardado.", alias);
                        }
                    }
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            match self.active_tab {
                UiTab::PluginsSkills => {
                    ui.heading("Gestor de Plugins Comunitarios, Propios y Skills");
                    ui.label(
                        "Actualiza plugins verificados de GitHub, instala/clona en local, crea los tuyos o añade skills.",
                    );
                    ui.add_space(6.0);

                    egui::ScrollArea::vertical()
                        .max_height((ui.available_height() - 32.0).max(180.0))
                        .show(ui, |ui| {
                            egui::Frame::group(ui.style()).show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new("Plugins verificados de GitHub y propios")
                                            .strong(),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui
                                                .add_enabled(
                                                    !self.is_refreshing_plugins,
                                                    egui::Button::new("🔄 Actualizar desde GitHub"),
                                                )
                                                .clicked()
                                            {
                                                self.trigger_refresh_plugins();
                                            }
                                        },
                                    );
                                });
                                ui.add_space(4.0);

                                let mut install_req: Option<CommunityPlugin> = None;
                                let mut activate_req: Option<CommunityPlugin> = None;
                                let mut open_req: Option<(String, String)> = None;

                                for plugin in &self.plugins_store.plugins {
                                    let is_installed =
                                        self.plugins_store.installed_ids.contains(&plugin.id);

                                    egui::Frame::group(ui.style()).show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.vertical(|ui| {
                                                ui.horizontal(|ui| {
                                                    ui.label(
                                                        egui::RichText::new(&plugin.name).strong(),
                                                    );
                                                    if plugin.verified {
                                                        ui.label(
                                                            egui::RichText::new("• Verificado GitHub")
                                                                .small(),
                                                        );
                                                    } else if plugin.custom {
                                                        ui.label(
                                                            egui::RichText::new("• Propio").small(),
                                                        );
                                                    }
                                                    if is_installed {
                                                        ui.label(
                                                            egui::RichText::new("• Instalado")
                                                                .small(),
                                                        );
                                                    }
                                                    if let Some(st) = plugin.stars {
                                                        ui.label(
                                                            egui::RichText::new(format!("★ {}", st))
                                                                .small(),
                                                        );
                                                    }
                                                });
                                                ui.label(
                                                    egui::RichText::new(&plugin.description)
                                                        .small(),
                                                );
                                                ui.label(
                                                    egui::RichText::new(format!(
                                                        "Proxy: {}",
                                                        plugin.proxy_endpoint
                                                    ))
                                                    .monospace()
                                                    .small(),
                                                );
                                                if !plugin.start_cmd.is_empty() {
                                                    ui.label(
                                                        egui::RichText::new(format!(
                                                            "Arranque: {}",
                                                            plugin.start_cmd
                                                        ))
                                                        .monospace()
                                                        .small(),
                                                    );
                                                }
                                            });

                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui.button("Activar en modelo").clicked() {
                                                        activate_req = Some(plugin.clone());
                                                    }
                                                    if !plugin.repo_url.is_empty() {
                                                        if ui.button("Instalar").clicked() {
                                                            install_req = Some(plugin.clone());
                                                        }
                                                        if ui.button("GitHub").clicked() {
                                                            open_req = Some((
                                                                plugin.name.clone(),
                                                                plugin.repo_url.clone(),
                                                            ));
                                                        }
                                                    }
                                                },
                                            );
                                        });
                                    });
                                    ui.add_space(4.0);
                                }

                                if let Some(p) = install_req {
                                    self.install_plugin_repo(&p);
                                }
                                if let Some(p) = activate_req {
                                    self.activate_plugin_on_selected_model(&p);
                                }
                                if let Some((n, u)) = open_req {
                                    self.open_provider_page(&n, &u);
                                }
                            });

                            ui.add_space(8.0);

                            egui::Frame::group(ui.style()).show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new("Implementar tu propio plugin").strong(),
                                );
                                ui.horizontal(|ui| {
                                    ui.label("Nombre:");
                                    ui.text_edit_singleline(&mut self.custom_plugin_name);
                                    ui.label("Repo/URL:");
                                    ui.text_edit_singleline(&mut self.custom_plugin_repo);
                                });
                                ui.horizontal(|ui| {
                                    ui.label("Endpoint proxy:");
                                    ui.text_edit_singleline(&mut self.custom_plugin_proxy);
                                    ui.label("Comando inicio:");
                                    ui.text_edit_singleline(&mut self.custom_plugin_cmd);
                                });
                                ui.horizontal(|ui| {
                                    ui.label("Descripción:");
                                    ui.text_edit_singleline(&mut self.custom_plugin_desc);
                                    if ui.button("💾 Guardar e instalar plugin").clicked() {
                                        self.add_custom_plugin();
                                    }
                                });
                            });

                            ui.add_space(8.0);

                            egui::Frame::group(ui.style()).show(ui, |ui| {
                                ui.label(egui::RichText::new("Instalar o crear Skills").strong());
                                ui.label(
                                    "Pega una URL de repositorio GitHub para clonar la skill o escribe/pega el contenido/resumen de SKILL.md:",
                                );
                                ui.horizontal(|ui| {
                                    ui.label("Nombre skill:");
                                    ui.text_edit_singleline(&mut self.skill_name_input);
                                    ui.label("Resumen:");
                                    ui.text_edit_singleline(&mut self.skill_summary_input);
                                });
                                ui.add(
                                    egui::TextEdit::multiline(&mut self.skill_repo_or_md_input)
                                        .desired_rows(2)
                                        .hint_text("https://github.com/usuario/mi-skill  o  instrucciones de la skill..."),
                                );
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    if ui.button("📥 Instalar Skill").clicked() {
                                        self.install_skill();
                                    }
                                    ui.label(
                                        egui::RichText::new(
                                            "Guardadas en ~/.config/free-ollama/skills/",
                                        )
                                        .small(),
                                    );
                                });

                                if !self.skills_store.skills.is_empty() {
                                    ui.separator();
                                    for sk in &self.skills_store.skills {
                                        ui.horizontal(|ui| {
                                            ui.label(egui::RichText::new(&sk.name).strong());
                                            ui.label(format!("— {}", sk.summary));
                                            ui.label(
                                                egui::RichText::new(&sk.local_path)
                                                    .monospace()
                                                    .small(),
                                            );
                                        });
                                    }
                                }
                            });
                        });
                }

                UiTab::Tokens => {
                    ui.heading("Conseguir e importar tokens (0 complicaciones)");
                    ui.label(
                        "1) Abre la consola del proveedor y copia tu API key.  2) Pégalos aquí y aplícalos en un clic.",
                    );
                    ui.add_space(6.0);

                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.label(egui::RichText::new("1. Conseguir tokens rápidos").strong());
                        ui.horizontal(|ui| {
                            if ui.button("🌐 OpenRouter Keys").clicked() {
                                self.open_provider_page(
                                    "OpenRouter Keys",
                                    "https://openrouter.ai/keys",
                                );
                            }
                            if ui.button("⚡ Groq Console").clicked() {
                                self.open_provider_page(
                                    "Groq Console",
                                    "https://console.groq.com/keys",
                                );
                            }
                            if ui.button("✨ Google AI Studio").clicked() {
                                self.open_provider_page(
                                    "Google AI Studio",
                                    "https://aistudio.google.com/app/apikey",
                                );
                            }
                        });
                        ui.label(
                            egui::RichText::new(
                                "Tip: puedes crear 2 o 3 keys y pegarlas juntas; el sistema rota automáticamente ante 401/403/429.",
                            )
                            .small(),
                        );
                    });

                    ui.add_space(8.0);

                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("2. Importación exprés (uno o varios tokens)")
                                .strong(),
                        );
                        ui.label(
                            "Pega tokens separados por coma, espacio o una línea por token:",
                        );
                        ui.add(
                            egui::TextEdit::multiline(&mut self.token_bulk_input)
                                .desired_rows(3)
                                .hint_text("sk-or-v1-..., gsk_..., o uno por línea"),
                        );

                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label("Aplicar a:");
                            ui.radio_value(
                                &mut self.token_scope,
                                TokenTargetScope::TodosLosModelos,
                                "Todos los modelos",
                            );
                            ui.radio_value(
                                &mut self.token_scope,
                                TokenTargetScope::ModeloActivo,
                                "Solo modelo activo",
                            );
                            ui.radio_value(
                                &mut self.token_scope,
                                TokenTargetScope::EndpointCoincidente,
                                "Por endpoint",
                            );
                        });

                        if self.token_scope == TokenTargetScope::EndpointCoincidente {
                            ui.horizontal(|ui| {
                                ui.label("Endpoint:");
                                ui.text_edit_singleline(&mut self.token_endpoint_filter);
                            });
                        }

                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if ui.button("✅ Añadir tokens").clicked() {
                                self.apply_bulk_tokens(false);
                            }
                            if ui.button("🔁 Reemplazar tokens").clicked() {
                                self.apply_bulk_tokens(true);
                            }
                            if ui.button("💬 Ir al chat").clicked() {
                                self.active_tab = UiTab::Chat;
                            }
                        });
                    });

                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("Estado de tokens por modelo").strong());

                    let mut aliases: Vec<String> = self.store.models.keys().cloned().collect();
                    aliases.sort();

                    egui::ScrollArea::vertical()
                        .max_height((ui.available_height() - 35.0).max(110.0))
                        .show(ui, |ui| {
                            if aliases.is_empty() {
                                ui.label("Aún no tienes modelos guardados.");
                            }
                            for alias in aliases {
                                if let Some(entry) = self.store.models.get(&alias) {
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(&alias).strong());
                                        ui.label(format!("— {} token(s)", entry.tokens.len()));
                                        ui.label(
                                            egui::RichText::new(&entry.endpoint).small(),
                                        );
                                    });
                                }
                            }
                        });
                }

                UiTab::Catalogo => {
                    ui.heading("Catálogo de modelos gratuitos");

                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.label(egui::RichText::new("Actualizar lista de modelos").strong());
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(
                                    !self.is_updating_catalog,
                                    egui::Button::new("🔄 Actualizar lista en vivo (OpenRouter)"),
                                )
                                .clicked()
                            {
                                self.trigger_live_catalog_update();
                            }

                            if ui
                                .add_enabled(
                                    !self.is_updating_catalog,
                                    egui::Button::new("Restaurar presets base"),
                                )
                                .clicked()
                            {
                                self.presets = default_presets();
                                let _ = save_catalog(&self.presets);
                                self.catalog_search.clear();
                                self.catalog_provider_filter = "Todos".to_string();
                                self.status_msg = format!(
                                    "Catálogo restaurado: {} presets base cargados.",
                                    self.presets.len()
                                );
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("O desde URL JSON:");
                            ui.text_edit_singleline(&mut self.custom_catalog_url);
                            if ui
                                .add_enabled(
                                    !self.is_updating_catalog,
                                    egui::Button::new("Cargar URL"),
                                )
                                .clicked()
                            {
                                self.trigger_url_catalog_update();
                            }
                        });
                    });

                    ui.add_space(6.0);

                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Tokens por defecto (coma):");
                            ui.text_edit_singleline(&mut self.default_import_tokens);
                        });
                        ui.horizontal(|ui| {
                            ui.label("Alias personalizado (opcional):");
                            ui.text_edit_singleline(&mut self.custom_import_alias);
                        });
                        ui.horizontal(|ui| {
                            ui.checkbox(
                                &mut self.import_with_billion_context,
                                "Activar plugin billion-context al importar",
                            );
                            ui.label("Proxy BC:");
                            ui.text_edit_singleline(&mut self.default_bc_url);
                        });
                        ui.horizontal(|ui| {
                            ui.label("Buscar:");
                            ui.text_edit_singleline(&mut self.catalog_search);
                            ui.label("Proveedor:");
                            egui::ComboBox::from_id_salt("provider_filter")
                                .selected_text(&self.catalog_provider_filter)
                                .show_ui(ui, |ui| {
                                    for p in ["Todos", "OpenRouter", "Groq"] {
                                        ui.selectable_value(
                                            &mut self.catalog_provider_filter,
                                            p.to_string(),
                                            p,
                                        );
                                    }
                                });
                            if ui.small_button("Limpiar filtros").clicked() {
                                self.catalog_search.clear();
                                self.catalog_provider_filter = "Todos".to_string();
                            }
                        });
                    });

                    let q = self.catalog_search.trim().to_lowercase();
                    let filtered: Vec<FreeModelPreset> = self
                        .presets
                        .iter()
                        .filter(|p| {
                            let provider_ok = self.catalog_provider_filter == "Todos"
                                || p.provider == self.catalog_provider_filter;
                            let search_ok = q.is_empty()
                                || p.alias.to_lowercase().contains(&q)
                                || p.remote_model.to_lowercase().contains(&q)
                                || p.provider.to_lowercase().contains(&q)
                                || p.notes.to_lowercase().contains(&q);
                            provider_ok && search_ok
                        })
                        .cloned()
                        .collect();

                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(format!("Mostrando {} modelos", filtered.len()));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Importar todos los filtrados").clicked() {
                                self.import_all_filtered(&filtered);
                            }
                        });
                    });
                    ui.separator();

                    let mut import_one: Option<(FreeModelPreset, bool)> = None;
                    let list_height = (ui.available_height() - 45.0).max(160.0);

                    egui::ScrollArea::vertical()
                        .max_height(list_height)
                        .show(ui, |ui| {
                            if filtered.is_empty() {
                                ui.label("No hay modelos que coincidan con el filtro actual.");
                            }
                            for preset in &filtered {
                                let installed = self.store.models.values().any(|m| {
                                    m.remote_model == preset.remote_model
                                        && m.endpoint == preset.endpoint
                                });

                                egui::Frame::group(ui.style()).show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.vertical(|ui| {
                                            ui.horizontal(|ui| {
                                                ui.label(
                                                    egui::RichText::new(&preset.alias).strong(),
                                                );
                                                ui.label(format!("[{}]", preset.provider));
                                                if installed {
                                                    ui.label(
                                                        egui::RichText::new("• Instalado")
                                                            .small(),
                                                    );
                                                }
                                            });
                                            ui.label(
                                                egui::RichText::new(&preset.remote_model).monospace(),
                                            );
                                            ui.label(egui::RichText::new(&preset.notes).small());
                                        });

                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if ui.button("Importar y chatear").clicked() {
                                                    import_one = Some((preset.clone(), true));
                                                }
                                                if ui.button("Importar").clicked() {
                                                    import_one = Some((preset.clone(), false));
                                                }
                                            },
                                        );
                                    });
                                });
                                ui.add_space(4.0);
                            }
                        });

                    if let Some((preset, go_chat)) = import_one {
                        self.import_preset(&preset, go_chat);
                    }
                }

                UiTab::Chat => {
                    if let Some(alias) = self.selected_alias.clone() {
                        let mut should_persist = false;
                        let mut should_clear_chat = false;
                        let mut new_status: Option<String> = None;

                        if let Some(entry) = self.store.models.get_mut(&alias) {
                            ui.horizontal(|ui| {
                                ui.heading(&alias);
                                ui.label(format!("— {}", entry.remote_model));
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.button("Limpiar chat").clicked() {
                                            should_clear_chat = true;
                                        }
                                        if ui.button("Rotar token").clicked()
                                            && !entry.tokens.is_empty()
                                        {
                                            entry.current_token_idx =
                                                (entry.current_token_idx + 1) % entry.tokens.len();
                                            should_persist = true;
                                            new_status = Some(format!(
                                                "Token activo ahora: #{} de {}",
                                                entry.current_token_idx + 1,
                                                entry.tokens.len()
                                            ));
                                        }
                                        ui.label(format!(
                                            "Token #{}/{}",
                                            if entry.tokens.is_empty() {
                                                0
                                            } else {
                                                entry.current_token_idx + 1
                                            },
                                            entry.tokens.len()
                                        ));
                                    },
                                );
                            });

                            ui.horizontal(|ui| {
                                if ui
                                    .checkbox(
                                        &mut entry.billion_context_enabled,
                                        "🧩 billion-context",
                                    )
                                    .changed()
                                {
                                    should_persist = true;
                                    new_status = Some(if entry.billion_context_enabled {
                                        format!(
                                            "billion-context activado para '{}' ({})",
                                            alias,
                                            effective_endpoint(entry)
                                        )
                                    } else {
                                        format!(
                                            "billion-context desactivado para '{}' ({})",
                                            alias, entry.endpoint
                                        )
                                    });
                                }
                                if entry.billion_context_enabled {
                                    ui.label("URL:");
                                    if ui
                                        .text_edit_singleline(&mut entry.billion_context_endpoint)
                                        .lost_focus()
                                    {
                                        should_persist = true;
                                    }
                                }
                            });

                            ui.horizontal(|ui| {
                                ui.label("Añadir tokens:");
                                ui.text_edit_singleline(&mut self.extra_tokens);
                                if ui.button("+").clicked() {
                                    let added = parse_tokens_flexible(&self.extra_tokens);
                                    if !added.is_empty() {
                                        entry.tokens.extend(added);
                                        self.extra_tokens.clear();
                                        let total = entry.tokens.len();
                                        should_persist = true;
                                        new_status = Some(format!(
                                            "Tokens actualizados en '{}' (total: {})",
                                            alias, total
                                        ));
                                    }
                                }
                            });
                        }

                        if should_clear_chat {
                            self.histories.remove(&alias);
                        }
                        if should_persist {
                            self.persist();
                        }
                        if let Some(msg) = new_status {
                            self.status_msg = msg;
                        }

                        ui.separator();

                        let available_height = ui.available_height() - 105.0;
                        let messages = self.histories.get(&alias).cloned().unwrap_or_default();

                        egui::ScrollArea::vertical()
                            .stick_to_bottom(true)
                            .max_height(available_height.max(120.0))
                            .show(ui, |ui| {
                                for msg in messages {
                                    let prefix = if msg.role == "user" {
                                        "Tú"
                                    } else {
                                        "Asistente"
                                    };
                                    egui::Frame::group(ui.style()).show(ui, |ui| {
                                        ui.label(egui::RichText::new(prefix).strong());
                                        ui.label(msg.content);
                                    });
                                    ui.add_space(6.0);
                                }
                            });

                        ui.separator();

                        ui.horizontal(|ui| {
                            let response = ui.add(
                                egui::TextEdit::multiline(&mut self.prompt_input)
                                    .desired_rows(2)
                                    .hint_text("Escribe tu mensaje..."),
                            );
                            let send_clicked = ui
                                .add_enabled(!self.is_loading, egui::Button::new("Enviar"))
                                .clicked();

                            if send_clicked
                                || (response.lost_focus()
                                    && ui.input(|i| {
                                        i.key_pressed(egui::Key::Enter) && !i.modifiers.shift
                                    }))
                            {
                                self.send_prompt();
                            }
                        });
                    } else {
                        ui.centered_and_justified(|ui| {
                            ui.vertical_centered(|ui| {
                                ui.label("No tienes ningún modelo seleccionado.");
                                ui.add_space(8.0);
                                if ui.button("Ir al catálogo de modelos gratuitos").clicked() {
                                    self.active_tab = UiTab::Catalogo;
                                }
                            });
                        });
                    }
                }
            }

            ui.separator();
            ui.horizontal(|ui| {
                if self.is_loading || self.is_updating_catalog || self.is_refreshing_plugins {
                    ui.spinner();
                }
                ui.label(&self.status_msg);
            });
        });
    }
}
