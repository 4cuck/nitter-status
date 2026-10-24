// SPDX-License-Identifier: AGPL-3.0-only
use std::sync::Arc;
use std::time::Instant;

use crate::Result;
use crate::ServerError;
use axum::response::IntoResponse;
use axum::{extract::State, response::Html};
use entities::state::scanner::ScannerConfig;
use entities::state::AppState;
use hyper::header;
use hyper::http::HeaderValue;
use hyper::HeaderMap;

fn insert_seo(
    context: &mut tera::Context,
    site_url: &str,
    path: &str,
    title: &str,
    description: &str,
) {
    let base = site_url.trim_end_matches('/');
    context.insert("site_url", base);
    context.insert("canonical", &format!("{base}{path}"));
    context.insert("page_title", title);
    context.insert("page_description", description);
}

fn media_q(header: &str, mime: &str) -> f32 {
    header
        .split(',')
        .find_map(|part| {
            let mut bits = part.split(';');
            let name = bits.next()?.trim();
            if !name.eq_ignore_ascii_case(mime) {
                return None;
            }
            let mut quality = 1.0;
            for param in bits {
                if let Some(value) = param.trim().strip_prefix("q=") {
                    quality = value.parse().unwrap_or(0.0);
                }
            }
            Some(quality)
        })
        .unwrap_or(0.0)
}

/// Cloudflare Markdown for Agents is a paid zone feature. Negotiate here so
/// agents can still request `Accept: text/markdown` on the free plan.
fn wants_markdown(headers: &HeaderMap) -> bool {
    let accept = headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let markdown = media_q(accept, "text/markdown");
    let html = media_q(accept, "text/html").max(media_q(accept, "application/xhtml+xml"));
    markdown > 0.0 && markdown > html
}

fn markdown_response(body: String, cache_control: &str) -> axum::response::Response {
    let mut res = body.into_response();
    let headers = res.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/markdown; charset=utf-8"),
    );
    headers.insert(header::VARY, HeaderValue::from_static("Accept"));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_str(cache_control).unwrap_or_else(|_| HeaderValue::from_static("public, max-age=900")),
    );
    res
}

pub async fn instances(
    State(ref app_state): State<AppState>,
    State(ref template): State<Arc<tera::Tera>>,
    State(ref config): State<Arc<crate::Config>>,
    headers: HeaderMap,
) -> Result<axum::response::Response> {
    if wants_markdown(&headers) {
        let guard = app_state
            .cache
            .read()
            .map_err(|_| ServerError::MutexFailure)?;
        let mut body = format!(
            "# Nitter Status\n\nLive status for working Nitter instances: nitter.cf, nitter.gq, xitter.cf, xitter.gq, teapaw.org, teapawt.net, teapawt.org, and goyimx.com.\n\nScraping is allowed on nitter.cf, nitter.gq, xitter.cf, xitter.gq, teapaw.org, teapawt.net, and teapawt.org. Ask the operator for permission before scraping any other Nitter instance.\n\nLast updated: {} UTC\n\n- JSON: /api/v1/instances\n- About: /about\n- Agent notes: /llms.txt\n\n| Instance | Frontend | Healthy | Average | All-time | RSS | Country |\n| --- | --- | --- | --- | --- | --- | --- |\n",
            guard.last_update.format("%Y-%m-%d %H:%M")
        );
        for host in &guard.hosts {
            let healthy = if host.is_bad_host {
                "unknown"
            } else if host.healthy {
                "yes"
            } else {
                "no"
            };
            let average = host
                .ping_avg
                .map(|value| format!("{value} ms"))
                .unwrap_or_else(|| "-".to_owned());
            let rss = if host.rss { "yes" } else { "no" };
            let frontend = host.version.as_deref().unwrap_or("-");
            body.push_str(&format!(
                "| [{domain}]({url}) | {frontend} | {healthy} | {average} | {pct}% | {rss} | {country} |\n",
                domain = host.domain,
                url = host.url,
                pct = host.healthy_percentage_overall,
                country = host.country.replace('|', "\\|"),
            ));
        }
        return Ok(markdown_response(
            body,
            &format!("public, max-age={}", config.max_age),
        ));
    }

    let mut context = tera::Context::new();
    insert_seo(
        &mut context,
        &config.site_url,
        "/",
        "Nitter Status — Working Nitter Instances",
        "Live Nitter status for working Nitter instances: nitter.cf, nitter.gq, xitter.cf, xitter.gq, teapaw.org, teapawt.net, teapawt.org, and goyimx.com. See which Nitter instance is up.",
    );
    let mut res = {
        let guard = app_state
            .cache
            .read()
            .map_err(|_| ServerError::MutexFailure)?;
        context.insert("instances", &guard.hosts);
        let time = guard.last_update.format("%Y.%m.%d %H:%M").to_string();
        context.insert("last_updated", &time);
        let start = Instant::now();
        let res = Html(template.render("instances.html.j2", &context)?).into_response();
        let end = Instant::now();
        drop(guard);
        let templating_time = end - start;
        tracing::trace!(templating_time = templating_time.as_millis());
        res
    };
    res.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_str(&format!("public, max-age={}", config.max_age)).unwrap(),
    );
    res.headers_mut()
        .insert(header::VARY, HeaderValue::from_static("Accept"));
    Ok(res)
}

pub async fn about(
    State(ref app_state): State<AppState>,
    State(ref template): State<Arc<tera::Tera>>,
    State(ref scanner_config): State<ScannerConfig>,
    headers: HeaderMap,
) -> Result<axum::response::Response> {
    if wants_markdown(&headers) {
        let latest_commit = app_state
            .cache
            .read()
            .map_err(|_| ServerError::MutexFailure)?
            .latest_commit
            .clone();
        let body = format!(
            "# About Nitter Status\n\nHow this Nitter instance list checks working Nitter instances.\n\nUptime check interval: {uptime}s.\n\nInstance list refresh interval: {wiki}s.\n\nResponse-time average window: {ping} hours.\n\nLatest known upstream commit: {latest_commit}\n\nChecked paths:\n\n- `{about}`\n- `{rss}`\n- `{profile}`\n- `{connectivity}`\n\nScraping is allowed on nitter.cf, nitter.gq, xitter.cf, xitter.gq, teapaw.org, teapawt.net, and teapawt.org. Ask the operator for permission before scraping any other Nitter instance.\n\nJSON API: `/api/v1/instances`\n\nAgent notes: `/llms.txt`\n\nSource: https://github.com/4cuck/nitter-status\n",
            uptime = scanner_config.instance_check_interval.as_secs(),
            wiki = scanner_config.list_fetch_interval.as_secs(),
            ping = scanner_config.ping_range.num_hours(),
            about = scanner_config.about_path,
            rss = scanner_config.rss_path,
            profile = scanner_config.profile_path,
            connectivity = scanner_config.connectivity_path,
        );
        return Ok(markdown_response(body, "public, max-age=900"));
    }

    let mut context = tera::Context::new();
    insert_seo(
        &mut context,
        &scanner_config.website_url,
        "/about",
        "About Nitter Status — Nitter Instance Checks",
        "How this Nitter status page checks working Nitter instances, how often the Nitter instance list refreshes, and where to read the JSON API.",
    );
    let mut paths = Vec::with_capacity(5);
    paths.push(&scanner_config.about_path);
    paths.push(&scanner_config.rss_path);
    paths.push(&scanner_config.profile_path);
    paths.push(&scanner_config.connectivity_path);
    context.insert("checked_paths", &paths);
    context.insert(
        "uptime_interval_s",
        &scanner_config.instance_check_interval.as_secs(),
    );
    context.insert(
        "wiki_interval_s",
        &scanner_config.list_fetch_interval.as_secs(),
    );
    context.insert(
        "ping_avg_interval_h",
        &scanner_config.ping_range.num_hours(),
    );
    {
        let guard = app_state
            .cache
            .read()
            .map_err(|_| ServerError::MutexFailure)?;
        context.insert("latest_commit", &guard.latest_commit);
    }

    let mut res = Html(template.render("about.html.j2", &context)?).into_response();
    res.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=900"),
    );
    res.headers_mut()
        .insert(header::VARY, HeaderValue::from_static("Accept"));
    Ok(res)
}

/// cnd website
pub async fn cnd_rip(
    State(ref template): State<Arc<tera::Tera>>,
    State(ref config): State<Arc<crate::Config>>,
    headers: HeaderMap,
) -> Result<axum::response::Response> {
    if wants_markdown(&headers) {
        let body = "# Cease and desist\n\nOn 24 August 2026 cease and desist letters were sent by X Corp. demanding a permanent takedown of Nitter instances and the project's repository.\n\nDevelopment stopped for the time being.\n\nThis status site still tracks which frontends answer: https://status.d420.net/\n".to_owned();
        return Ok(markdown_response(body, "public, max-age=900"));
    }

    let mut context = tera::Context::new();
    insert_seo(
        &mut context,
        &config.site_url,
        "/rip",
        "Nitter cease and desist",
        "On 24 August 2026 X Corp. sent cease and desist letters demanding a takedown of Nitter. This page records that notice.",
    );
    let mut res = Html(template.render("rip_cnd.html.j2", &context)?).into_response();
    res.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=900"),
    );
    res.headers_mut()
        .insert(header::VARY, HeaderValue::from_static("Accept"));
    Ok(res)
}
