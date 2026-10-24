// SPDX-License-Identifier: AGPL-3.0-only
//! Pass the public Bagel proof-of-work interstitial some frontends show
//! before the real page. The site publishes the solver as WebAssembly.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use reqwest::Client;
use wasmi::{Engine as WasmEngine, Linker, Module, Store};

use crate::FetchError;

const SOLVE_LIMIT: Duration = Duration::from_secs(5);

struct Challenge<'a> {
    payload: &'a str,
    verify: &'a str,
    solver: &'a str,
}

pub fn is_challenge(body: &str) -> bool {
    body.contains("<bagel-challenge") && body.contains("data-p=")
}

fn solve_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

pub async fn lock() -> tokio::sync::MutexGuard<'static, ()> {
    solve_lock().lock().await
}

pub async fn solve(client: &Client, page_url: &str, html: &str) -> Result<(), FetchError> {
    let challenge = parse_challenge(html)
        .ok_or_else(|| FetchError::Bagel("challenge page is missing its payload".to_owned()))?;
    let page = reqwest::Url::parse(page_url)
        .map_err(|_| FetchError::Bagel(format!("invalid page url {page_url}")))?;
    let solver_url = page
        .join(challenge.solver)
        .map_err(|_| FetchError::Bagel("invalid solver url".to_owned()))?;
    let verify_url = page
        .join(challenge.verify)
        .map_err(|_| FetchError::Bagel("invalid verify url".to_owned()))?;
    let wasm = wasm_bytes(client, solver_url.as_str()).await?;
    let payload = challenge.payload.to_owned();
    let proof = tokio::task::spawn_blocking(move || prove(&wasm, &payload))
        .await
        .map_err(|error| FetchError::Bagel(error.to_string()))?
        .map_err(FetchError::Bagel)?;
    let response = client
        .post(verify_url)
        .header(reqwest::header::CONTENT_TYPE, "text/plain")
        .body(proof)
        .send()
        .await?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(FetchError::Bagel(format!("verify failed ({status}): {body}")));
    }
    Ok(())
}

fn parse_challenge(html: &str) -> Option<Challenge<'_>> {
    let tag_start = html.find("<bagel-challenge")?;
    let tag_end = html[tag_start..].find('>')?;
    let tag = &html[tag_start..tag_start + tag_end];
    Some(Challenge {
        payload: attr(tag, "data-p")?,
        verify: attr(tag, "data-v")?,
        solver: attr(tag, "data-s")?,
    })
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=\"");
    let start = tag.find(&key)? + key.len();
    let end = tag[start..].find('"')?;
    Some(&tag[start..start + end])
}

async fn wasm_bytes(client: &Client, url: &str) -> Result<Vec<u8>, FetchError> {
    {
        let cache = wasm_cache().lock().unwrap();
        if let Some(bytes) = cache.get(url) {
            return Ok(bytes.clone());
        }
    }
    let response = client.get(url).send().await?;
    if !response.status().is_success() {
        return Err(FetchError::Bagel(format!(
            "solver download failed ({})",
            response.status()
        )));
    }
    let bytes = response.bytes().await?.to_vec();
    wasm_cache()
        .lock()
        .unwrap()
        .insert(url.to_owned(), bytes.clone());
    Ok(bytes)
}

fn wasm_cache() -> &'static Mutex<HashMap<String, Vec<u8>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Vec<u8>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn prove(wasm: &[u8], payload: &str) -> Result<String, String> {
    let handoff = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|error| format!("challenge payload: {error}"))?;
    let engine = WasmEngine::default();
    let module = Module::new(&engine, wasm).map_err(|error| format!("solver module: {error}"))?;
    let mut store = Store::new(&engine, ());
    let linker = Linker::new(&engine);
    let instance = linker
        .instantiate(&mut store, &module)
        .map_err(|error| format!("solver instantiate: {error}"))?
        .start(&mut store)
        .map_err(|error| format!("solver start: {error}"))?;
    let memory = instance
        .get_memory(&store, "memory")
        .ok_or_else(|| "solver has no memory".to_owned())?;
    let buf = instance
        .get_typed_func::<(), i32>(&store, "buf")
        .map_err(|error| error.to_string())?;
    let unpack = instance
        .get_typed_func::<i32, i32>(&store, "unpack")
        .map_err(|error| error.to_string())?;
    let solve = instance
        .get_typed_func::<(i64, i32), i64>(&store, "solve")
        .map_err(|error| error.to_string())?;
    let seal = instance
        .get_typed_func::<(i64, i32, i32, i32, i32), i32>(&store, "seal")
        .map_err(|error| error.to_string())?;

    let base = buf.call(&mut store, ()).map_err(|error| error.to_string())? as usize;
    memory
        .write(&mut store, base, &handoff)
        .map_err(|error| format!("solver memory: {error}"))?;
    let levels = unpack
        .call(&mut store, handoff.len() as i32)
        .map_err(|error| error.to_string())?;
    if levels < 0 {
        return Err("invalid challenge handoff".to_owned());
    }
    let cpu_level = levels & 0xff;
    let gpu_level = levels >> 8;
    if gpu_level > 0 && gpu_level == cpu_level {
        return Err("challenge requires WebGPU".to_owned());
    }

    let started = Instant::now();
    let mut nonce = 0i64;
    let mut found = -1i64;
    let mut batch = 16i32;
    while found < 0 {
        let before = Instant::now();
        found = solve
            .call(&mut store, (nonce, batch))
            .map_err(|error| error.to_string())?;
        nonce = nonce.saturating_add(i64::from(batch));
        let took = before.elapsed().as_millis().max(1) as i32;
        batch = ((batch.saturating_mul(40)) / took).clamp(1, 1 << 20);
        if started.elapsed() > SOLVE_LIMIT {
            return Err("challenge proof took too long".to_owned());
        }
    }

    let iv = random_u32()?;
    let length = seal
        .call(&mut store, (found, iv as i32, 0, 0, cpu_level))
        .map_err(|error| error.to_string())?;
    if length < 0 {
        return Err("solver seal failed".to_owned());
    }
    let proof = memory
        .data(&store)
        .get(base..base + length as usize)
        .ok_or_else(|| "solver proof is out of range".to_owned())?;
    Ok(URL_SAFE_NO_PAD.encode(proof))
}

fn random_u32() -> Result<u32, String> {
    let mut bytes = [0u8; 4];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| std::io::Read::read_exact(&mut file, &mut bytes))
        .map_err(|error| format!("random iv: {error}"))?;
    Ok(u32::from_le_bytes(bytes))
}
