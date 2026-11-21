//! Внешний каталог стратегий Zaprett: index.json → manifests → artifacts.
//!
//! Данные не входят в поставку wireproxy-control: пользователь загружает их
//! по явному действию из указанного репозитория. Каждый элемент показывает
//! автора и URL источника, артефакты проверяются по sha256.
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::state::AppState;

const MAX_INDEX_BYTES: u64 = 4 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 512 * 1024;
const MAX_STRATEGY_BYTES: u64 = 4 * 1024 * 1024;
const MAX_BIN_BYTES: u64 = 16 * 1024 * 1024;
const MAX_LIST_BYTES: u64 = 48 * 1024 * 1024;
const MAX_ITEMS: usize = 300;
const MAX_DEPENDENCIES: usize = 64;
const MAX_DEPTH: usize = 6;
const META_FILE: &str = "catalog-meta.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CatalogIndex {
    #[serde(default)]
    pub schema: i64,
    #[serde(default)]
    pub items: Vec<IndexItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IndexItem {
    #[serde(default)]
    pub id: String,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub manifest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Manifest {
    #[serde(default)]
    pub schema: i64,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub artifact: Artifact,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Artifact {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CatalogMeta {
    #[serde(default)]
    source: String,
    #[serde(default)]
    updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RefreshReport {
    pub updated: usize,
    pub failed: usize,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadReport {
    pub strategies: usize,
    pub failed: usize,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CatalogTestResults {
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub started_at: String,
    #[serde(default)]
    pub finished_at: String,
    #[serde(default)]
    pub done: usize,
    #[serde(default)]
    pub total: usize,
    #[serde(default)]
    pub current: String,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub results: Vec<CatalogTestResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CatalogTestResult {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub author: String,
    pub reachable: Vec<String>,
    pub codes: Vec<CatalogTestCode>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CatalogTestCode {
    pub domain: String,
    pub code: String,
    pub ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct TestPayload {
    domains: Vec<String>,
    timeout_secs: u64,
    strategies: Vec<TestPayloadStrategy>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct TestPayloadStrategy {
    id: String,
    name: String,
    kind: String,
    author: String,
    args: Vec<String>,
}

pub fn catalog_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("zapret-catalog")
}

fn manifests_dir(data_dir: &Path) -> PathBuf {
    catalog_dir(data_dir).join("manifests")
}

fn files_dir(data_dir: &Path) -> PathBuf {
    catalog_dir(data_dir).join("files")
}

fn index_path(data_dir: &Path) -> PathBuf {
    catalog_dir(data_dir).join("index.json")
}

fn meta_path(data_dir: &Path) -> PathBuf {
    catalog_dir(data_dir).join(META_FILE)
}

fn merged_hostlist_path(data_dir: &Path) -> PathBuf {
    catalog_dir(data_dir).join("merged-hostlist.txt")
}

fn merged_ipset_path(data_dir: &Path) -> PathBuf {
    catalog_dir(data_dir).join("merged-ipset.txt")
}

fn test_payload_path(data_dir: &Path) -> PathBuf {
    catalog_dir(data_dir).join("test-payload.json")
}

fn test_results_path(data_dir: &Path) -> PathBuf {
    catalog_dir(data_dir).join("test-results.json")
}

fn test_cmd_path(data_dir: &Path) -> PathBuf {
    catalog_dir(data_dir).join("catalog-test.cmd")
}

fn safe_id(id: &str) -> Result<&str> {
    let id = id.trim();
    if id.is_empty()
        || id.len() > 96
        || id.contains("..")
        || !id.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'_' || byte == b'-'
        })
    {
        bail!("некорректный id каталога");
    }
    Ok(id)
}

fn file_extension(url: &reqwest::Url, kind: &str) -> String {
    let candidate = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .and_then(|name| name.rsplit_once('.').map(|(_, ext)| ext))
        .unwrap_or("");
    if (1..=8).contains(&candidate.len()) && candidate.chars().all(|ch| ch.is_ascii_alphanumeric())
    {
        return format!(".{candidate}");
    }
    match kind {
        "bin" => ".bin".to_string(),
        "lua_lib" => ".lua".to_string(),
        _ => ".txt".to_string(),
    }
}

fn cached_file(data_dir: &Path, id: &str) -> Option<PathBuf> {
    let files = files_dir(data_dir);
    let prefix = format!("{id}.");
    let entries = std::fs::read_dir(files).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(&prefix) && !name.ends_with(".tmp") {
            return Some(entry.path());
        }
    }
    None
}

fn load_index(data_dir: &Path) -> Option<CatalogIndex> {
    let data = std::fs::read(index_path(data_dir)).ok()?;
    serde_json::from_slice(&data).ok()
}

fn load_manifests(data_dir: &Path) -> HashMap<String, Manifest> {
    let mut out = HashMap::new();
    let Ok(entries) = std::fs::read_dir(manifests_dir(data_dir)) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|v| v.to_str()) != Some("json") {
            continue;
        }
        let Ok(data) = std::fs::read(&path) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_slice::<Manifest>(&data) else {
            continue;
        };
        if !manifest.id.is_empty() {
            out.insert(manifest.id.clone(), manifest);
        }
    }
    out
}

fn load_meta(data_dir: &Path) -> CatalogMeta {
    std::fs::read(meta_path(data_dir))
        .ok()
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_default()
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn source_page(source: &str) -> String {
    if let Ok(url) = reqwest::Url::parse(source) {
        if url.host_str() == Some("raw.githubusercontent.com") {
            let segments: Vec<&str> = url
                .path_segments()
                .map(|items| items.filter(|item| !item.is_empty()).collect())
                .unwrap_or_default();
            if segments.len() >= 2 {
                return format!("https://github.com/{}/{}", segments[0], segments[1]);
            }
        }
    }
    source.to_string()
}

fn allowed_host(host: &str) -> bool {
    host == "raw.githubusercontent.com"
        || host == "github.com"
        || host.ends_with(".githubusercontent.com")
}

fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_unspecified()
                || v4.is_multicast()
                || v4.is_documentation()
                || o[0] == 0
                || o[0] >= 240
                || (o[0] == 100 && (64..=127).contains(&o[1]))
                || (o[0] == 192 && o[1] == 0 && o[2] == 0)
                || (o[0] == 198 && (o[1] == 18 || o[1] == 19)))
        }
        IpAddr::V6(v6) => {
            let segments = v6.segments();
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (segments[0] & 0xfe00) == 0xfc00
                || (segments[0] & 0xffc0) == 0xfe80
                || (segments[0] == 0x2001 && segments[1] == 0x0db8)
                || v6
                    .to_ipv4_mapped()
                    .map(|mapped| !public_ip(IpAddr::V4(mapped)))
                    .unwrap_or(false))
        }
    }
}

fn validate_url(raw: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(raw).context("некорректный URL каталога")?;
    if url.scheme() != "https" {
        bail!("каталог должен загружаться только по https");
    }
    if !url.username().is_empty() || url.password().is_some() {
        bail!("URL каталога не должен содержать логин/пароль");
    }
    let host = url.host_str().context("в URL каталога нет хоста")?;
    if !allowed_host(host) {
        bail!("недопустимый хост каталога: {host}");
    }
    if let Some(port) = url.port() {
        if port != 443 {
            bail!("недопустимый порт каталога: {port}");
        }
    }
    Ok(url)
}

async fn ensure_public_host(host: &str) -> Result<()> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        if !public_ip(ip) {
            bail!("каталог указывает на непубличный адрес");
        }
        return Ok(());
    }
    let addrs = tokio::net::lookup_host((host, 443))
        .await
        .with_context(|| format!("не удалось разрешить {host}"))?;
    let mut any = false;
    for addr in addrs {
        any = true;
        if !public_ip(addr.ip()) {
            bail!("каталог указывает на непубличный адрес {host}");
        }
    }
    if !any {
        bail!("не удалось разрешить {host}");
    }
    Ok(())
}

fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 || validate_url(attempt.url().as_str()).is_err() {
                attempt.stop()
            } else {
                attempt.follow()
            }
        }))
        .user_agent(format!(
            "wireproxy-control/{} zaprett-catalog",
            env!("CARGO_PKG_VERSION")
        ))
        .build()?)
}

async fn get_bytes(raw: &str, max: u64) -> Result<Vec<u8>> {
    let url = validate_url(raw)?;
    let host = url.host_str().context("в URL нет хоста")?;
    ensure_public_host(host).await?;
    let client = http_client()?;
    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("не удалось загрузить {raw}"))?;
    let final_url = response.url().clone();
    let final_parsed = validate_url(final_url.as_str())?;
    let final_host = final_parsed.host_str().context("в URL нет хоста")?;
    ensure_public_host(final_host).await?;
    let response = response
        .error_for_status()
        .context("сервер каталога отдал ошибку")?;
    if let Some(length) = response.content_length() {
        if length > max {
            bail!("файл каталога больше лимита");
        }
    }
    let bytes = response.bytes().await.context("обрыв загрузки каталога")?;
    if bytes.len() as u64 > max {
        bail!("файл каталога больше лимита");
    }
    Ok(bytes.to_vec())
}

async fn fetch_manifest(item: &IndexItem) -> Result<Manifest> {
    let id = safe_id(&item.id)?;
    let bytes = get_bytes(&item.manifest, MAX_MANIFEST_BYTES).await?;
    let mut manifest: Manifest =
        serde_json::from_slice(&bytes).with_context(|| format!("некорректный манифест {id}"))?;
    // В репозитории встречаются id с точкой на конце; ключом каталога
    // остаётся id из index.json, чтобы не ломать cache/UI.
    manifest.id = id.to_string();
    Ok(manifest)
}

fn supported_kind(kind: &str) -> bool {
    matches!(
        kind,
        "nfqws"
            | "nfqws2"
            | "list"
            | "list_exclude"
            | "ipset"
            | "ipset_exclude"
            | "bin"
            | "lua_lib"
    )
}

pub async fn refresh(state: &Arc<AppState>) -> Result<RefreshReport> {
    use std::sync::OnceLock;
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    let _guard = LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    let settings = state.settings_snapshot();
    let source = settings.zapret_catalog_url.clone();
    let bytes = get_bytes(&source, MAX_INDEX_BYTES).await?;
    let index: CatalogIndex =
        serde_json::from_slice(&bytes).context("некорректный index.json каталога")?;
    if index.items.len() > MAX_ITEMS {
        bail!("в каталоге больше {MAX_ITEMS} элементов");
    }
    let items: Vec<IndexItem> = index
        .items
        .iter()
        .filter(|item| supported_kind(&item.kind))
        .cloned()
        .collect();
    let mut seen = HashSet::new();
    for item in &items {
        let id = safe_id(&item.id)?;
        validate_url(&item.manifest)?;
        if !seen.insert(id.to_string()) {
            bail!("дубликат id каталога: {id}");
        }
    }
    let data_dir = state.dir.clone();
    write_atomic(&index_path(&data_dir), &bytes)?;
    std::fs::create_dir_all(manifests_dir(&data_dir))?;
    std::fs::create_dir_all(files_dir(&data_dir))?;

    let results: Vec<Result<()>> = futures_util::stream::iter(items.into_iter().map(|item| {
        let data_dir = data_dir.clone();
        async move {
            let manifest = fetch_manifest(&item).await?;
            let path = manifests_dir(&data_dir).join(format!("{}.json", item.id));
            let encoded = serde_json::to_vec_pretty(&manifest)?;
            write_atomic(&path, &encoded)?;
            Ok(())
        }
    }))
    .buffer_unordered(8)
    .collect()
    .await;

    let mut updated = 0usize;
    let mut errors = Vec::new();
    for result in results {
        match result {
            Ok(()) => updated += 1,
            Err(error) => errors.push(format!("{error:#}")),
        }
    }
    let meta = CatalogMeta {
        source: source.clone(),
        updated_at: chrono::Utc::now().to_rfc3339(),
    };
    write_atomic(&meta_path(&data_dir), &serde_json::to_vec_pretty(&meta)?)?;
    if updated == 0 && !errors.is_empty() {
        bail!(
            "не удалось загрузить ни один манифест: {}",
            errors.first().cloned().unwrap_or_default()
        );
    }
    Ok(RefreshReport {
        updated,
        failed: errors.len(),
        errors: errors.into_iter().take(5).collect(),
    })
}

fn max_artifact_bytes(kind: &str) -> u64 {
    match kind {
        "bin" => MAX_BIN_BYTES,
        "lua_lib" => 4 * 1024 * 1024,
        "nfqws" | "nfqws2" => MAX_STRATEGY_BYTES,
        _ => MAX_LIST_BYTES,
    }
}

async fn ensure_artifact(data_dir: &Path, item: &IndexItem, manifest: &Manifest) -> Result<()> {
    let id = safe_id(&item.id)?;
    if manifest.artifact.url.is_empty() {
        bail!("у {id} нет artifact.url");
    }
    let sha = manifest.artifact.sha256.trim().to_ascii_lowercase();
    if sha.len() != 64 || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("у {id} некорректный sha256");
    }
    let url = validate_url(&manifest.artifact.url)?;
    let extension = file_extension(&url, &item.kind);
    let target = files_dir(data_dir).join(format!("{id}{extension}"));
    if target.is_file() {
        let data = std::fs::read(&target)?;
        let actual = hex::encode(Sha256::digest(&data));
        if actual.eq_ignore_ascii_case(&sha) {
            return Ok(());
        }
        let _ = std::fs::remove_file(&target);
    }
    let bytes = get_bytes(&manifest.artifact.url, max_artifact_bytes(&item.kind)).await?;
    let actual = hex::encode(Sha256::digest(&bytes));
    if !actual.eq_ignore_ascii_case(&sha) {
        bail!("sha256 {id} не совпадает");
    }
    std::fs::create_dir_all(files_dir(data_dir))?;
    // Удаляем старый файл с другим расширением, чтобы cached_file не путался.
    for entry in std::fs::read_dir(files_dir(data_dir))?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(&format!("{id}.")) && entry.path() != target {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    write_atomic(&target, &bytes)?;
    Ok(())
}

pub async fn download(state: &Arc<AppState>, id: &str) -> Result<usize> {
    let id = safe_id(id)?.to_string();
    let data_dir = state.dir.clone();
    let index = load_index(&data_dir).context("сначала обновите каталог")?;
    let mut index_by_id: HashMap<String, IndexItem> = HashMap::new();
    let mut index_by_url: HashMap<String, IndexItem> = HashMap::new();
    for item in &index.items {
        if !supported_kind(&item.kind) {
            continue;
        }
        index_by_id.insert(item.id.clone(), item.clone());
        index_by_url.insert(item.manifest.clone(), item.clone());
    }
    if !index_by_id.contains_key(&id) {
        bail!("стратегии {id} нет в каталоге");
    }
    let mut manifests = load_manifests(&data_dir);
    let mut queue: Vec<(IndexItem, usize)> = vec![(index_by_id[&id].clone(), 0)];
    let mut visited = HashSet::new();
    let mut downloaded = 0usize;
    while let Some((item, depth)) = queue.pop() {
        if depth > MAX_DEPTH || visited.len() >= MAX_DEPENDENCIES {
            bail!("слишком глубокая или большая цепочка зависимостей");
        }
        if !visited.insert(item.id.clone()) {
            continue;
        }
        let manifest = if let Some(manifest) = manifests.get(&item.id) {
            manifest.clone()
        } else {
            let manifest = fetch_manifest(&item).await?;
            let path = manifests_dir(&data_dir).join(format!("{}.json", item.id));
            write_atomic(&path, &serde_json::to_vec_pretty(&manifest)?)?;
            manifests.insert(item.id.clone(), manifest.clone());
            manifest
        };
        ensure_artifact(&data_dir, &item, &manifest).await?;
        downloaded += 1;
        for dependency in &manifest.dependencies {
            let Some(dep_item) = index_by_url.get(dependency) else {
                bail!("зависимость {dependency} отсутствует в index.json");
            };
            queue.push((dep_item.clone(), depth + 1));
        }
    }
    Ok(downloaded)
}

/// Скачивает все стратегии nfqws/nfqws2 и их зависимости. Нужно для
/// пакетной проверки: «Обновить каталог» загружает только манифесты.
pub async fn download_all_strategies(state: &Arc<AppState>) -> Result<DownloadReport> {
    let index = load_index(&state.dir).context("сначала обновите каталог")?;
    let ids: Vec<String> = index
        .items
        .iter()
        .filter(|item| matches!(item.kind.as_str(), "nfqws" | "nfqws2"))
        .map(|item| item.id.clone())
        .collect();
    if ids.is_empty() {
        bail!("в каталоге нет стратегий");
    }
    let mut strategies = 0usize;
    let mut errors = Vec::new();
    for id in ids {
        match download(state, &id).await {
            Ok(_) => strategies += 1,
            Err(e) => errors.push(format!("{id}: {e:#}")),
        }
    }
    Ok(DownloadReport {
        strategies,
        failed: errors.len(),
        errors: errors.into_iter().take(5).collect(),
    })
}

pub fn select(state: &Arc<AppState>, lists: &[String], ipsets: &[String]) -> Result<()> {
    let data_dir = &state.dir;
    let index = load_index(data_dir).context("сначала обновите каталог")?;
    let manifests = load_manifests(data_dir);
    let kinds: HashMap<&str, &str> = index
        .items
        .iter()
        .filter(|item| supported_kind(&item.kind))
        .map(|item| (item.id.as_str(), item.kind.as_str()))
        .collect();
    let mut clean_lists = Vec::new();
    for id in lists {
        let id = safe_id(id)?;
        if kinds.get(id) != Some(&"list") {
            bail!("{id} не является списком доменов");
        }
        if !manifests.contains_key(id) {
            bail!("список {id} не загружен");
        }
        if !clean_lists.iter().any(|value: &String| value == id) {
            clean_lists.push(id.to_string());
        }
    }
    let mut clean_ipsets = Vec::new();
    for id in ipsets {
        let id = safe_id(id)?;
        if kinds.get(id) != Some(&"ipset") {
            bail!("{id} не является ipset");
        }
        if !manifests.contains_key(id) {
            bail!("ipset {id} не загружен");
        }
        if !clean_ipsets.iter().any(|value: &String| value == id) {
            clean_ipsets.push(id.to_string());
        }
    }
    let mut settings = state
        .settings
        .lock()
        .map_err(|_| anyhow::anyhow!("настройки повреждены"))?;
    settings.zapret_catalog_lists = clean_lists;
    settings.zapret_catalog_ipsets = clean_ipsets;
    settings.save(&state.dir)?;
    Ok(())
}

fn merged_file(data_dir: &Path, ids: &[String], target: &Path, header: &str) -> Result<bool> {
    if ids.is_empty() {
        return Ok(false);
    }
    let manifests = load_manifests(data_dir);
    let mut content = String::new();
    content.push_str(header);
    content.push('\n');
    for id in ids {
        let manifest = manifests
            .get(id)
            .with_context(|| format!("манифест {id} не загружен"))?;
        let path = cached_file(data_dir, id).with_context(|| format!("файл {id} не скачан"))?;
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("не удалось прочитать {}", path.display()))?;
        for line in text.lines() {
            let line = line.trim();
            if !line.is_empty() {
                content.push_str(line);
                content.push('\n');
            }
        }
        if !manifest.description.is_empty() {
            content.push_str(&format!(
                "# source: {}\n",
                manifest.description.replace('\n', " ")
            ));
        }
    }
    write_atomic(target, content.as_bytes())?;
    Ok(true)
}

fn placeholder_path(path: &Path) -> Result<String> {
    let text = path.to_string_lossy();
    if text.contains('"') {
        bail!("путь каталога содержит недопустимую кавычку");
    }
    Ok(format!("\"{text}\""))
}

fn placeholder_value(
    name: &str,
    data_dir: &Path,
    manifests: &HashMap<String, Manifest>,
    merged_hosts: bool,
    merged_ipsets: bool,
) -> Result<String> {
    if name == "hostlists" {
        return if merged_hosts {
            Ok(format!(
                "--hostlist={}",
                placeholder_path(&merged_hostlist_path(data_dir))?
            ))
        } else {
            Ok(String::new())
        };
    }
    if name == "ipsets" {
        return if merged_ipsets {
            Ok(format!(
                "--ipset={}",
                placeholder_path(&merged_ipset_path(data_dir))?
            ))
        } else {
            Ok(String::new())
        };
    }
    let (kind, id) = name
        .split_once(':')
        .with_context(|| format!("неизвестная переменная стратегии: ${{{name}}}"))?;
    let id = safe_id(id)?;
    let manifest = manifests
        .get(id)
        .with_context(|| format!("манифест {id} не загружен"))?;
    let path = cached_file(data_dir, id).with_context(|| format!("файл {id} не скачан"))?;
    match kind {
        "bin" => Ok(placeholder_path(&path)?),
        "lua_lib" => Ok(placeholder_path(&path)?),
        "hostlist" => Ok(format!("--hostlist={}", placeholder_path(&path)?)),
        "hostlist_exclude" => Ok(format!("--hostlist-exclude={}", placeholder_path(&path)?)),
        "ipset" => Ok(format!("--ipset={}", placeholder_path(&path)?)),
        "ipset_exclude" => Ok(format!("--ipset-exclude={}", placeholder_path(&path)?)),
        _ => {
            let _ = manifest;
            bail!("неизвестная переменная стратегии: ${{{name}}}")
        }
    }
}

fn expand_strategy(
    data_dir: &Path,
    text: &str,
    manifests: &HashMap<String, Manifest>,
    settings: &crate::settings::Settings,
) -> Result<String> {
    let merged_hosts = merged_file(
        data_dir,
        &settings.zapret_catalog_lists,
        &merged_hostlist_path(data_dir),
        "# wireproxy-control: merged Zaprett hostlists",
    )?;
    let merged_ipsets = merged_file(
        data_dir,
        &settings.zapret_catalog_ipsets,
        &merged_ipset_path(data_dir),
        "# wireproxy-control: merged Zaprett ipsets",
    )?;
    let mut out = String::new();
    let mut rest = text.trim_start_matches('\u{feff}');
    while let Some(pos) = rest.find("${") {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 2..];
        let Some(end) = after.find('}') else {
            bail!("незакрытая переменная в стратегии");
        };
        let name = &after[..end];
        out.push_str(&placeholder_value(
            name,
            data_dir,
            manifests,
            merged_hosts,
            merged_ipsets,
        )?);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn valid_port_spec(value: &str) -> bool {
    let parts: Vec<&str> = value.split('-').collect();
    if parts.len() > 2 || parts.iter().any(|part| part.is_empty()) {
        return false;
    }
    parts
        .iter()
        .all(|part| part.parse::<u16>().map(|port| port > 0).unwrap_or(false))
}

fn add_ports(out: &mut Vec<String>, value: &str) {
    for part in value.split(',') {
        let part = part.trim();
        if valid_port_spec(part) && !out.iter().any(|have| have == part) {
            out.push(part.to_string());
        }
    }
}

fn derive_wf_args(args: &[String]) -> Vec<String> {
    if args.iter().any(|arg| arg.starts_with("--wf-")) {
        return Vec::new();
    }
    let mut tcp = Vec::new();
    let mut udp = Vec::new();
    let mut quic = false;
    for arg in args {
        if let Some(value) = arg.strip_prefix("--filter-tcp=") {
            add_ports(&mut tcp, value);
        } else if let Some(value) = arg.strip_prefix("--filter-udp=") {
            add_ports(&mut udp, value);
        } else if arg == "--filter-l7=quic" || arg.starts_with("--filter-l7=quic,") {
            quic = true;
        }
    }
    if tcp.is_empty() && udp.is_empty() {
        tcp = vec!["80".to_string(), "443".to_string()];
        udp = vec!["443".to_string()];
    } else if udp.is_empty() && quic {
        udp.push("443".to_string());
    }
    let mut out = Vec::new();
    if !tcp.is_empty() {
        out.push(format!("--wf-tcp={}", tcp.join(",")));
    }
    if !udp.is_empty() {
        out.push(format!("--wf-udp={}", udp.join(",")));
    }
    out
}

fn strategy_text(data_dir: &Path, id: &str) -> Result<String> {
    let path = cached_file(data_dir, id).with_context(|| format!("файл {id} не скачан"))?;
    std::fs::read_to_string(&path)
        .with_context(|| format!("не удалось прочитать {}", path.display()))
}

pub struct PreparedStrategy {
    pub item: IndexItem,
    pub manifest: Manifest,
    pub bin: PathBuf,
    pub args: Vec<String>,
}

pub fn prepare(state: &AppState, id: &str) -> Result<PreparedStrategy> {
    let id = safe_id(id)?.to_string();
    let data_dir = &state.dir;
    let index = load_index(data_dir).context("каталог не загружен")?;
    let item = index
        .items
        .iter()
        .find(|item| item.id == id && supported_kind(&item.kind))
        .context("стратегия не найдена")?
        .clone();
    if !matches!(item.kind.as_str(), "nfqws" | "nfqws2") {
        bail!("это не стратегия Zapret");
    }
    let manifests = load_manifests(data_dir);
    let manifest = manifests
        .get(&id)
        .context("манифест стратегии не найден")?
        .clone();
    let settings = state.settings_snapshot();
    let expanded = expand_strategy(
        data_dir,
        &strategy_text(data_dir, &id)?,
        &manifests,
        &settings,
    )?;
    let mut args = crate::zapret::preset::split_args(&expanded);
    if args.is_empty() {
        bail!("пустая стратегия каталога");
    }
    if args.iter().any(|arg| !arg.starts_with("--")) {
        bail!("стратегия каталога содержит недопустимые аргументы");
    }
    if !args.iter().any(|arg| arg.starts_with("--filter-")) {
        bail!("в стратегии каталога нет фильтров");
    }
    let mut full = derive_wf_args(&args);
    if !args.iter().any(|arg| arg == "--wf-tcp-empty=1") {
        full.insert(0, "--wf-tcp-empty=1".to_string());
    }
    full.append(&mut args);
    let bin = match item.kind.as_str() {
        "nfqws" => crate::zapret::preset::engine_path_v1(data_dir)?,
        "nfqws2" => crate::zapret::preset::engine_path_v2(data_dir)?,
        _ => unreachable!(),
    };
    Ok(PreparedStrategy {
        item,
        manifest,
        bin,
        args: full,
    })
}

pub fn preview_json(state: &AppState, id: &str) -> Result<Value> {
    let prepared = prepare(state, id)?;
    Ok(json!({
        "id": prepared.item.id,
        "kind": prepared.item.kind,
        "name": prepared.manifest.name,
        "author": prepared.manifest.author,
        "source": prepared.item.manifest,
        "bin": prepared.bin.display().to_string(),
        "args": prepared.args,
    }))
}

pub async fn apply(state: &Arc<AppState>, id: &str) -> Result<()> {
    #[cfg(not(windows))]
    {
        let _ = (state, id);
        bail!("Zapret поддерживается только на Windows");
    }
    #[cfg(windows)]
    {
        let id = safe_id(id)?.to_string();
        download(state, &id).await?;
        let prepared = prepare(state, &id)?;
        crate::zapret::service::install_args_for(&state.dir, &prepared.bin, prepared.args)?;
        let mut settings = state
            .settings
            .lock()
            .map_err(|_| anyhow::anyhow!("настройки повреждены"))?;
        settings.zapret_catalog_strategy = id.clone();
        settings.save(&state.dir)?;
        crate::wlog!(
            "zapret: применяется стратегия каталога {} ({}, автор {})",
            prepared.manifest.name,
            prepared.item.kind,
            prepared.manifest.author
        );
        Ok(())
    }
}

fn clean_test_domains(domains: &[String]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for raw in domains {
        let Some(host) = crate::lists::validate_hostname(raw) else {
            bail!("некорректный домен для проверки: {raw}");
        };
        if host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") {
            bail!("для проверки нужен публичный домен, а не {host}");
        }
        if !out.iter().any(|item| item == &host) {
            out.push(host);
        }
    }
    if out.is_empty() {
        out.push("www.youtube.com".to_string());
        out.push("discord.com".to_string());
    }
    if out.len() > 8 {
        bail!("слишком много доменов для проверки");
    }
    Ok(out)
}

fn load_test_results(data_dir: &Path) -> Option<CatalogTestResults> {
    let data = std::fs::read(test_results_path(data_dir)).ok()?;
    serde_json::from_slice(&data).ok()
}

fn save_test_results(data_dir: &Path, results: &CatalogTestResults) -> Result<()> {
    write_atomic(
        &test_results_path(data_dir),
        &serde_json::to_vec_pretty(results)?,
    )
}

pub async fn start_test(state: &Arc<AppState>, domains: &[String]) -> Result<()> {
    #[cfg(not(windows))]
    {
        let _ = (state, domains);
        bail!("Zapret поддерживается только на Windows");
    }
    #[cfg(windows)]
    {
        if load_test_results(&state.dir)
            .map(|r| r.running)
            .unwrap_or(false)
        {
            bail!("проверка стратегий уже выполняется");
        }
        let domains = clean_test_domains(domains)?;
        let index = load_index(&state.dir).context("каталог не загружен")?;
        let mut strategies = Vec::new();
        for item in &index.items {
            if !matches!(item.kind.as_str(), "nfqws" | "nfqws2") {
                continue;
            }
            if cached_file(&state.dir, &item.id).is_none() {
                continue;
            }
            let Ok(prepared) = prepare(state, &item.id) else {
                continue;
            };
            strategies.push(TestPayloadStrategy {
                id: prepared.item.id.clone(),
                name: if prepared.manifest.name.is_empty() {
                    prepared.item.id.clone()
                } else {
                    prepared.manifest.name.clone()
                },
                kind: prepared.item.kind.clone(),
                author: prepared.manifest.author.clone(),
                args: prepared.args,
            });
        }
        if strategies.is_empty() {
            bail!("нет скачанных стратегий: скачайте их в каталоге (или нажмите «Скачать все стратегии»)");
        }
        strategies.truncate(64);
        let payload = TestPayload {
            domains: domains.clone(),
            timeout_secs: 5,
            strategies,
        };
        write_atomic(
            &test_payload_path(&state.dir),
            &serde_json::to_vec_pretty(&payload)?,
        )?;
        let total = payload.strategies.len();
        let initial = CatalogTestResults {
            running: true,
            started_at: chrono::Utc::now().to_rfc3339(),
            finished_at: String::new(),
            done: 0,
            total,
            current: String::new(),
            error: None,
            results: Vec::new(),
        };
        save_test_results(&state.dir, &initial)?;
        let exe = crate::paths::normalize_path(std::env::current_exe()?);
        let dir = crate::paths::normalize_path(state.dir.clone());
        let cmd = format!(
            "@echo off\r\n\"{}\" --data-dir \"{}\" catalog-test\r\n",
            exe.display(),
            dir.display()
        );
        write_atomic(&test_cmd_path(&state.dir), cmd.as_bytes())?;
        crate::wlog!(
            "zapret: проверка {} стратегий каталога на {} доменах",
            total,
            domains.len()
        );
        if let Err(e) = crate::shell::run_elevated(&test_cmd_path(&state.dir)) {
            let mut failed = initial;
            failed.running = false;
            failed.error = Some(format!("{e:#}"));
            failed.finished_at = chrono::Utc::now().to_rfc3339();
            let _ = save_test_results(&state.dir, &failed);
            return Err(e);
        }
        Ok(())
    }
}

pub fn reset_test(state: &AppState, force: bool) -> Result<()> {
    if !force
        && load_test_results(&state.dir)
            .map(|r| r.running)
            .unwrap_or(false)
    {
        bail!("проверка ещё выполняется");
    }
    let _ = std::fs::remove_file(test_results_path(&state.dir));
    let _ = std::fs::remove_file(test_payload_path(&state.dir));
    let _ = std::fs::remove_file(test_cmd_path(&state.dir));
    Ok(())
}

#[cfg(windows)]
fn probe_domain(
    runtime: &tokio::runtime::Runtime,
    client: &reqwest::Client,
    domain: &str,
    timeout_secs: u64,
) -> CatalogTestCode {
    let started = std::time::Instant::now();
    let timeout = Duration::from_secs(timeout_secs.clamp(2, 15));
    let url = format!("https://{domain}/");
    let code = runtime.block_on(async {
        match tokio::time::timeout(timeout, client.get(&url).send()).await {
            Ok(Ok(response)) => response.status().as_u16().to_string(),
            _ => "000".to_string(),
        }
    });
    CatalogTestCode {
        domain: domain.to_string(),
        code,
        ms: started.elapsed().as_millis() as u64,
    }
}

/// Запускается elevated из start_test; читает фиксированный payload из
/// каталога и прогоняет winws/winws2 по очереди с HTTP-пробами.
pub fn run_test_helper(data_dir: &Path) -> Result<()> {
    #[cfg(not(windows))]
    {
        let _ = data_dir;
        bail!("Zapret поддерживается только на Windows");
    }
    #[cfg(windows)]
    {
        let payload: TestPayload = serde_json::from_slice(
            &std::fs::read(test_payload_path(data_dir)).context("нет payload проверки")?,
        )
        .context("некорректный payload проверки")?;
        if payload.strategies.is_empty() || payload.domains.is_empty() {
            bail!("пустой payload проверки");
        }
        if payload.strategies.len() > 64 || payload.domains.len() > 8 {
            bail!("слишком большой payload проверки");
        }
        let mut results = CatalogTestResults {
            running: true,
            started_at: chrono::Utc::now().to_rfc3339(),
            finished_at: String::new(),
            done: 0,
            total: payload.strategies.len(),
            current: String::new(),
            error: None,
            results: Vec::new(),
        };
        save_test_results(data_dir, &results)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("не удалось создать runtime проверки")?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(20))
            .user_agent(format!(
                "wireproxy-control/{} catalog-test",
                env!("CARGO_PKG_VERSION")
            ))
            .build()
            .context("не удалось создать HTTP-клиент проверки")?;
        let was_running = crate::zapret::service::query(data_dir).running;
        if was_running {
            if let Err(e) = crate::zapret::service::stop() {
                results.running = false;
                results.error = Some(format!("не удалось остановить службу Zapret: {e:#}"));
                results.finished_at = chrono::Utc::now().to_rfc3339();
                let _ = save_test_results(data_dir, &results);
                return Err(e);
            }
            std::thread::sleep(Duration::from_millis(1500));
        }
        let outcome = (|| -> Result<()> {
            for (index, strategy) in payload.strategies.iter().enumerate() {
                if strategy.args.is_empty()
                    || strategy.args.len() > 256
                    || strategy
                        .args
                        .iter()
                        .any(|arg| !arg.starts_with("--") || arg.contains(['\r', '\n']))
                {
                    results.results.push(CatalogTestResult {
                        id: strategy.id.clone(),
                        name: strategy.name.clone(),
                        kind: strategy.kind.clone(),
                        author: strategy.author.clone(),
                        reachable: Vec::new(),
                        codes: Vec::new(),
                        error: Some("недопустимые аргументы стратегии".to_string()),
                    });
                    results.done = index + 1;
                    save_test_results(data_dir, &results)?;
                    continue;
                }
                results.current = strategy.id.clone();
                save_test_results(data_dir, &results)?;
                let bin = match strategy.kind.as_str() {
                    "nfqws" => crate::zapret::preset::engine_path_v1(data_dir),
                    "nfqws2" => crate::zapret::preset::engine_path_v2(data_dir),
                    _ => {
                        bail!("неизвестный движок стратегии");
                    }
                };
                let mut result = CatalogTestResult {
                    id: strategy.id.clone(),
                    name: strategy.name.clone(),
                    kind: strategy.kind.clone(),
                    author: strategy.author.clone(),
                    reachable: Vec::new(),
                    codes: Vec::new(),
                    error: None,
                };
                match bin {
                    Ok(bin) => {
                        let child = Command::new(&bin)
                            .args(&strategy.args)
                            .creation_flags(0x08000000)
                            .stdin(Stdio::null())
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .spawn();
                        match child {
                            Ok(mut child) => {
                                std::thread::sleep(Duration::from_millis(1800));
                                for domain in &payload.domains {
                                    let code = probe_domain(
                                        &runtime,
                                        &client,
                                        domain,
                                        payload.timeout_secs,
                                    );
                                    if code.code != "000" {
                                        result.reachable.push(domain.clone());
                                    }
                                    result.codes.push(code);
                                }
                                let _ = child.kill();
                                let _ = child.wait();
                            }
                            Err(e) => result.error = Some(format!("winws не запустился: {e}")),
                        }
                    }
                    Err(e) => result.error = Some(format!("{e:#}")),
                }
                results.results.push(result);
                results.done = index + 1;
                save_test_results(data_dir, &results)?;
                std::thread::sleep(Duration::from_millis(300));
            }
            Ok(())
        })();
        if was_running {
            let _ = crate::zapret::service::start();
        }
        results.running = false;
        results.current.clear();
        results.finished_at = chrono::Utc::now().to_rfc3339();
        if let Err(e) = &outcome {
            results.error = Some(format!("{e:#}"));
        }
        save_test_results(data_dir, &results)?;
        outcome
    }
}

pub fn status_json(state: &AppState) -> Value {
    let settings = state.settings_snapshot();
    let data_dir = &state.dir;
    let index = load_index(data_dir);
    let manifests = load_manifests(data_dir);
    let meta = load_meta(data_dir);
    let test = load_test_results(data_dir);
    let mut index_by_url: HashMap<String, IndexItem> = HashMap::new();
    if let Some(index) = &index {
        for item in &index.items {
            if supported_kind(&item.kind) {
                index_by_url.insert(item.manifest.clone(), item.clone());
            }
        }
    }
    let engine_v1 = crate::zapret::preset::engine_path_v1(data_dir).is_ok();
    let engine_v2 = crate::zapret::preset::engine_path_v2(data_dir).is_ok();
    let mut strategies = Vec::new();
    let mut lists = Vec::new();
    let mut ipsets = Vec::new();
    let mut bins = Vec::new();
    if let Some(index) = &index {
        for item in &index.items {
            if !supported_kind(&item.kind) {
                continue;
            }
            let Some(manifest) = manifests.get(&item.id) else {
                continue;
            };
            let downloaded = cached_file(data_dir, &item.id).is_some();
            let deps_ready = manifest.dependencies.iter().all(|url| {
                index_by_url
                    .get(url)
                    .and_then(|dep| cached_file(data_dir, &dep.id))
                    .is_some()
            });
            let supported = match item.kind.as_str() {
                "nfqws" => engine_v1,
                "nfqws2" => engine_v2,
                _ => false,
            };
            let entry = json!({
                "id": item.id,
                "kind": item.kind,
                "name": if manifest.name.is_empty() { &item.id } else { &manifest.name },
                "description": manifest.description,
                "author": manifest.author,
                "version": manifest.version,
                "source": item.manifest,
                "artifact": manifest.artifact.url,
                "downloaded": downloaded,
                "ready": downloaded && deps_ready,
                "active": settings.zapret_catalog_strategy == item.id,
                "supported": supported,
                "dependencies": manifest.dependencies.len(),
            });
            match item.kind.as_str() {
                "nfqws" | "nfqws2" => strategies.push(entry),
                "list" | "list_exclude" => lists.push(entry),
                "ipset" | "ipset_exclude" => ipsets.push(entry),
                "bin" | "lua_lib" => bins.push(entry),
                _ => {}
            }
        }
    }
    json!({
        "source": settings.zapret_catalog_url,
        "sourcePage": source_page(&settings.zapret_catalog_url),
        "updatedAt": meta.updated_at,
        "cached": index.is_some(),
        "engineV1": engine_v1,
        "engineV2": engine_v2,
        "selectedLists": settings.zapret_catalog_lists,
        "selectedIpsets": settings.zapret_catalog_ipsets,
        "active": settings.zapret_catalog_strategy,
        "strategies": strategies,
        "lists": lists,
        "ipsets": ipsets,
        "bins": bins,
        "test": test,
        "licenseNote": "Данные загружаются из внешнего репозитория; у zaprett-repo нет явной лицензии. Указывайте источник и проверяйте права перед использованием.",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_https_and_foreign_hosts() {
        assert!(validate_url("http://raw.githubusercontent.com/a/b").is_err());
        assert!(validate_url("https://example.com/index.json").is_err());
        assert!(validate_url("https://raw.githubusercontent.com/a/b").is_ok());
        assert!(validate_url("https://raw.githubusercontent.com:8443/a/b").is_err());
    }

    #[test]
    fn rejects_private_addresses() {
        assert!(!public_ip("127.0.0.1".parse().unwrap()));
        assert!(!public_ip("10.0.0.1".parse().unwrap()));
        assert!(!public_ip("192.168.1.1".parse().unwrap()));
        assert!(!public_ip("169.254.1.1".parse().unwrap()));
        assert!(!public_ip("::1".parse().unwrap()));
        assert!(!public_ip("fc00::1".parse().unwrap()));
        assert!(!public_ip("fe80::1".parse().unwrap()));
        assert!(public_ip("8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn parses_manifests_and_safe_ids() {
        let manifest: Manifest = serde_json::from_str(
            r#"{"schema":1,"id":"strategy-default","name":"x","version":"1","author":"a","description":"d","dependencies":[],"artifact":{"url":"https://raw.githubusercontent.com/x/y","sha256":"00"}}"#,
        )
        .unwrap();
        assert_eq!(manifest.id, "strategy-default");
        assert!(safe_id("strategy-default").is_ok());
        assert!(safe_id("../evil").is_err());
        assert!(safe_id("a/b").is_err());
    }

    #[test]
    fn expands_placeholders() {
        let dir = std::env::temp_dir().join(format!("wpc-catalog-test-{}", std::process::id()));
        std::fs::create_dir_all(manifests_dir(&dir)).unwrap();
        std::fs::create_dir_all(files_dir(&dir)).unwrap();
        std::fs::write(files_dir(&dir).join("tls.bin"), b"x").unwrap();
        std::fs::write(files_dir(&dir).join("list-youtube.txt"), "youtube.com\n").unwrap();
        std::fs::write(manifests_dir(&dir).join("tls.json"), br#"{"id":"tls"}"#).unwrap();
        std::fs::write(
            manifests_dir(&dir).join("list-youtube.json"),
            br#"{"id":"list-youtube"}"#,
        )
        .unwrap();
        let manifests = load_manifests(&dir);
        let settings = crate::settings::Settings {
            zapret_catalog_lists: vec!["list-youtube".to_string()],
            ..Default::default()
        };
        let text =
            "--filter-tcp=443 --dpi-desync-fake-tls=${bin:tls} ${hostlist:list-youtube} --new";
        let expanded = expand_strategy(&dir, text, &manifests, &settings).unwrap();
        assert!(expanded.contains("tls.bin"));
        assert!(expanded.contains("list-youtube.txt"));
        assert!(!expanded.contains("${"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn derives_windivert_ports() {
        let args = vec![
            "--filter-tcp=80,443".to_string(),
            "--filter-udp=443,50000-50100".to_string(),
            "--dpi-desync=fake".to_string(),
        ];
        let wf = derive_wf_args(&args);
        assert!(wf.iter().any(|arg| arg == "--wf-tcp=80,443"));
        assert!(wf.iter().any(|arg| arg == "--wf-udp=443,50000-50100"));
    }
}
