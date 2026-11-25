//! Автоматический выбор самого быстрого Xray-конфига среди сохранённых
//! конфигов и рабочего пула подписок.
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::{bail, Result};

use crate::state::AppState;
use crate::vless::{configs, pool};
use crate::wlog;

/// Сколько кандидатов из пула проверять живым трафиком за один проход.
/// Свежий тест пула мог устареть, поэтому одного «лучшего» недостаточно.
const POOL_CANDIDATES: usize = 5;

struct Candidate {
    name: String,
    ms: u64,
    kind: Kind,
    link: Option<String>,
}

enum Kind {
    Managed(String),
    Pool(usize),
}

fn vless_healthy(state: &AppState) -> bool {
    state.health_snapshot().1
}

struct PoolCandidate {
    index: usize,
    name: String,
    link: String,
    ms: u64,
}

fn pool_candidates(state: &Arc<AppState>) -> Vec<PoolCandidate> {
    let Ok(pool) = state.pool.lock() else {
        return Vec::new();
    };
    let mut items: Vec<_> = pool
        .entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.ok && entry.ms.is_some())
        .collect();
    items.sort_by_key(|(_, entry)| entry.ms.unwrap_or(u64::MAX));
    let mut seen = HashSet::new();
    items
        .into_iter()
        .filter(|(_, entry)| seen.insert(entry.link.clone()))
        .take(POOL_CANDIDATES)
        .map(|(index, entry)| PoolCandidate {
            index,
            name: entry.name.clone(),
            link: entry.link.clone(),
            ms: entry.ms.unwrap_or(u64::MAX),
        })
        .collect()
}

/// Проверяет сохранённые Xray/Outline-конфиги и сравнивает их с рабочими
/// конфигами пула. Возвращает true, если активный конфиг изменился.
pub async fn select_fastest(state: &Arc<AppState>) -> Result<bool> {
    let settings = state.settings_snapshot();
    if !settings.xray_enabled {
        return Ok(false);
    }
    if configs::test_busy() {
        return Ok(false);
    }
    if pool::test_busy() || state.pool.lock().map(|pool| pool.testing).unwrap_or(false) {
        return Ok(false);
    }

    let active_slug = configs::active_slug(&state.dir);
    let items = configs::list(&state.dir)?;
    let managed: Vec<_> = items
        .into_iter()
        .filter(|item| {
            item.valid
                && (item.slug != configs::POOL_SLUG
                    || active_slug.as_deref() == Some(item.slug.as_str()))
        })
        .collect();
    let pool_candidates = pool_candidates(state);
    if managed.is_empty() && pool_candidates.is_empty() {
        return Ok(false);
    }

    let active_link = state
        .pool
        .lock()
        .ok()
        .and_then(|pool| pool.active_link.clone());
    // У активного Xray может быть уже установленное соединение, которое
    // переживает блокировку новых TLS/WS-сессий. Поэтому свежий тест
    // временным процессом не должен объявлять рабочий канал мёртвым.
    let vless_ok = vless_healthy(state);

    let mut candidates: Vec<Candidate> = Vec::new();
    let mut active_pool_ok = false;
    for item in &managed {
        match configs::test(&state.dir, &item.slug).await {
            Ok((ms, ip)) => {
                wlog!("авто-конфиг Xray: «{}» — {ms} мс, IP {ip}", item.name);
                if item.slug == configs::POOL_SLUG {
                    active_pool_ok = true;
                }
                candidates.push(Candidate {
                    name: item.name.clone(),
                    ms,
                    kind: Kind::Managed(item.slug.clone()),
                    link: None,
                });
            }
            Err(e) => wlog!("авто-конфиг Xray: «{}» — {e:#}", item.name),
        }
    }
    for entry in pool_candidates {
        candidates.push(Candidate {
            name: format!("{} (пул)", entry.name),
            ms: entry.ms,
            kind: Kind::Pool(entry.index),
            link: Some(entry.link),
        });
    }
    candidates.sort_by_key(|item| item.ms);

    let Some(best) = candidates.first() else {
        if vless_healthy(state) {
            wlog!(
                "авто-конфиг Xray: свежая проверка не прошла, но VLESS-канал работает — оставляю активный"
            );
            return Ok(false);
        }
        bail!("ни один Xray-конфиг не прошёл проверку трафика");
    };
    let best_is_active = match &best.kind {
        Kind::Managed(slug) => active_slug.as_deref() == Some(slug.as_str()),
        // Ссылка из пула считается активной, только если копия активного
        // конфига тоже прошла проверку. Иначе канал может быть мёртв, а
        // сохранённая задержка пула — устаревшей.
        Kind::Pool(_) => {
            (active_pool_ok || vless_ok)
                && match (&best.link, &active_link) {
                    (Some(link), Some(active)) => link == active,
                    _ => false,
                }
        }
    };
    if best_is_active {
        wlog!(
            "авто-конфиг Xray: активный «{}» уже самый быстрый ({} мс)",
            best.name,
            best.ms
        );
        return Ok(false);
    }
    if !state.settings_snapshot().auto_rotate {
        return Ok(false);
    }

    // Кандидаты уже отсортированы по скорости. Сохранённые задержки пула
    // могли устареть, поэтому каждого проверяем живым трафиком и при
    // неудаче переходим к следующему, а не ждём 15 минут до таймера.
    for candidate in candidates {
        match candidate.kind {
            Kind::Managed(slug) => {
                if active_slug.as_deref() == Some(slug.as_str()) {
                    wlog!("авто-конфиг Xray: оставляю активный «{}»", candidate.name);
                    return Ok(false);
                }
                configs::activate_auto(&state.dir, &slug)?;
                state.xr.restart();
                configs::log_active(&state.dir, &slug);
                wlog!(
                    "авто-конфиг Xray: выбран «{}» — {} мс",
                    candidate.name,
                    candidate.ms
                );
                return Ok(true);
            }
            Kind::Pool(index) => {
                if (active_pool_ok || vless_healthy(state))
                    && match (&candidate.link, &active_link) {
                        (Some(link), Some(active)) => link == active,
                        _ => false,
                    }
                {
                    wlog!("авто-конфиг Xray: оставляю активный «{}»", candidate.name);
                    return Ok(false);
                }
                match pool::apply_entry_checked(state, index).await {
                    Ok(_) => {
                        wlog!(
                            "авто-конфиг Xray: выбран «{}» — {} мс",
                            candidate.name,
                            candidate.ms
                        );
                        return Ok(true);
                    }
                    Err(e) => wlog!(
                        "авто-конфиг Xray: кандидат «{}» не подошёл — {e:#}",
                        candidate.name
                    ),
                }
            }
        }
    }
    if vless_healthy(state) {
        wlog!(
            "авто-конфиг Xray: свежая проверка кандидатов не прошла, но VLESS-канал работает — оставляю активный"
        );
        return Ok(false);
    }
    bail!("ни один Xray-конфиг не прошёл проверку трафика")
}
