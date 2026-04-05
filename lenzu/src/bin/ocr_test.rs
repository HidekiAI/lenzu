//! OCR smoke-test binary.
//!
//! Sends assets/Unit-test-sample-texts.png to one or both backends (local
//! ollama and remote OpenRouter) using the actual `OcrClient` from client.rs —
//! the same code path the lenzu application uses.
//!
//! Build:   cargo build -p lenzu --bin ocr-test
//! Run:     ./target/release/ocr-test [options]
//! Via sh:  scripts/test-ocr.sh [options]  (wrapper that builds + runs this)
//!
//! Flags:
//!   --ollama-endpoint URL    (default: http://localhost:11434/v1/chat/completions)
//!   --ollama-model NAME      (repeatable — each model is tested in order)
//!                            (default: gemma4:e2b)
//!   --remote-endpoint URL    (default: https://openrouter.ai/api/v1/chat/completions)
//!   --remote-model NAME      (default: google/gemini-2.0-flash-001)
//!   --remote-key KEY         (default: $OPENROUTER_API_KEY env var)
//!   --image PATH             (default: <repo>/assets/Unit-test-sample-texts.png)
//!   --expected PATH          (default: <repo>/assets/Unit-test-sample-texts.json)
//!   --max-dim N              longest-edge pixel cap before encoding (default: 640)
//!   --timeout N              per-backend timeout in seconds (default: 30)
//!   --no-timeout             disable timeout (for CPU inference timing)
//!   --num-ctx N              ollama num_ctx option (default: 2048; 0 = disable)
//!   --all-local              test full production chain: gemma4 → glm-ocr → moondream → qwen2.5vl:3b + remote
//!   --skip-ollama            skip local-ollama test
//!   --skip-remote            skip remote-OpenRouter test
//!
//! Exit codes:
//!   0 — all checks passed
//!   1 — one or more checks failed or a backend was unreachable

use image::imageops::FilterType;
use lenzu::{
    client::OcrClient,
    config::AppConfig,
    utils::{encode_as_grayscale, encode_for_fallback},
};
use serde::Deserialize;
use std::time::Instant;

#[derive(Deserialize)]
struct SampleEntry {
    label: String,
    text: String,
    top_xy: [i64; 2],
    bot_xy: [i64; 2],
}

struct Config {
    ollama_endpoint: String,
    /// Ordered list of local models to test.  Each is tested independently.
    /// Populated by --ollama-model (repeatable) or --all-local.
    /// Defaults to [primary model from AppConfig].
    ollama_models: Vec<String>,
    remote_endpoint: String,
    remote_model: String,
    remote_key: String,
    image_path: std::path::PathBuf,
    expected_path: std::path::PathBuf,
    max_dim: u32,
    /// `None` = no timeout (--no-timeout); `Some(n)` = abort after n seconds.
    timeout_secs: Option<u64>,
    num_ctx: Option<u32>,
    skip_ollama: bool,
    skip_remote: bool,
}

fn parse_args() -> Config {
    let defaults = AppConfig::default();
    let mut cfg = Config {
        ollama_endpoint: defaults.llm_api_endpoint,
        ollama_models: vec![defaults.llm_default_model],
        remote_endpoint: defaults.fallback_llm_api_endpoint,
        remote_model: defaults.fallback_llm_model,
        remote_key: std::env::var("OPENROUTER_API_KEY").unwrap_or_default(),
        image_path: std::path::PathBuf::new(),
        expected_path: std::path::PathBuf::new(),
        max_dim: 640,
        timeout_secs: Some(60),
        num_ctx: Some(2048),
        skip_ollama: false,
        skip_remote: false,
    };

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--ollama-endpoint" => cfg.ollama_endpoint = args.next().expect("--ollama-endpoint needs a value"),
            // --ollama-model may be given multiple times; first use replaces the default.
            "--ollama-model" => {
                let m = args.next().expect("--ollama-model needs a value");
                if cfg.ollama_models == vec![AppConfig::default().llm_default_model] {
                    cfg.ollama_models = vec![m]; // replace default on first use
                } else {
                    cfg.ollama_models.push(m);   // append on subsequent uses
                }
            }
            "--remote-endpoint" => cfg.remote_endpoint = args.next().expect("--remote-endpoint needs a value"),
            "--remote-model"    => cfg.remote_model    = args.next().expect("--remote-model needs a value"),
            "--remote-key"      => cfg.remote_key      = args.next().expect("--remote-key needs a value"),
            "--image"           => cfg.image_path      = args.next().expect("--image needs a value").into(),
            "--expected"        => cfg.expected_path   = args.next().expect("--expected needs a value").into(),
            "--max-dim"  => cfg.max_dim = args.next().expect("--max-dim needs a value").parse().expect("--max-dim must be a number"),
            "--timeout"  => {
                let n: u64 = args.next().expect("--timeout needs a value").parse().expect("--timeout must be a number");
                cfg.timeout_secs = Some(n);
            }
            "--no-timeout"  => cfg.timeout_secs = None,
            "--num-ctx"     => {
                let n: u32 = args.next().expect("--num-ctx needs a value").parse().expect("--num-ctx must be a number");
                cfg.num_ctx = if n == 0 { None } else { Some(n) };
            }
            "--skip-ollama"  => cfg.skip_ollama = true,
            "--skip-remote"  => cfg.skip_remote = true,
            // --all-local: expand to the full production fallback chain
            // (primary model + any local_fallback_models from AppConfig defaults).
            "--all-local" => {
                let d = AppConfig::default();
                let mut models = vec![d.llm_default_model];
                models.extend(d.local_fallback_models);
                // If the production config has no fallbacks defined, add the known
                // OCR-specialist models so the test is still useful.
                // Ordered by expected quality/speed: glm-ocr (OCR-specialist),
                // moondream (lightweight vision), qwen2.5vl:3b (vision-language).
                // Florence-2 excluded: not in ollama registry (HuggingFace/Python only).
                if models.len() == 1 {
                    models.push("glm-ocr".to_string());
                    models.push("moondream".to_string());
                    models.push("qwen2.5vl:3b".to_string());
                }
                cfg.ollama_models = models;
            }
            other => { eprintln!("Unknown argument: {other}"); std::process::exit(1); }
        }
    }

    // Derive asset paths relative to the binary location if not provided.
    // Binary is at <repo>/target/{release|debug}/ocr-test  → repo = 3 parents up.
    if cfg.image_path.as_os_str().is_empty() || cfg.expected_path.as_os_str().is_empty() {
        let repo_root = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().and_then(|p| p.parent()).and_then(|p| p.parent()).map(|p| p.to_path_buf()))
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        if cfg.image_path.as_os_str().is_empty() {
            cfg.image_path = repo_root.join("assets/Unit-test-sample-texts.png");
        }
        if cfg.expected_path.as_os_str().is_empty() {
            cfg.expected_path = repo_root.join("assets/Unit-test-sample-texts.json");
        }
    }

    cfg
}

// ── ollama pre-flight ─────────────────────────────────────────────────────────

/// Extract base URL from an API endpoint, e.g.
/// "http://localhost:11434/v1/chat/completions" → "http://localhost:11434"
fn ollama_base(endpoint: &str) -> String {
    if let Some(pos) = endpoint.find("/v1/") {
        endpoint[..pos].to_string()
    } else if let Some(pos) = endpoint.find("/api/") {
        endpoint[..pos].to_string()
    } else {
        endpoint.trim_end_matches('/').to_string()
    }
}

fn ollama_preflight(endpoint: &str, model: &str) -> Result<(), String> {
    let http = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;

    let base = ollama_base(endpoint);

    // Health check
    http.get(format!("{}/", base))
        .send()
        .map_err(|e| format!("ollama not running at {base} — {e}\n       Start it (or run scripts/run.sh) before running this test."))?;

    // Model check
    let tags: serde_json::Value = http
        .get(format!("{}/api/tags", base))
        .send()
        .map_err(|e| format!("cannot reach {base}/api/tags — {e}"))?
        .json()
        .map_err(|e| format!("cannot parse /api/tags response — {e}"))?;

    let found = tags["models"]
        .as_array()
        .map(|arr| arr.iter().any(|m| {
            m["name"].as_str()
                .map(|n| n == model || n.starts_with(&format!("{}:", model)))
                .unwrap_or(false)
        }))
        .unwrap_or(false);

    if !found {
        return Err(format!(
            "model '{model}' not found in ollama.\n       Run: ollama pull {model}"
        ));
    }
    Ok(())
}

// ── ollama runner cancellation ────────────────────────────────────────────────
// When the client times out, the ollama runner subprocess keeps consuming GPU/CPU
// until the generation completes.  Sending SIGKILL to it stops it immediately;
// ollama serve will respawn a fresh runner on the next request.
fn cancel_ollama_runner() {
    let status = std::process::Command::new("pkill")
        .args(["-KILL", "-f", "ollama runner.*--model"])
        .status();
    if status.map(|s| s.success()).unwrap_or(false) {
        println!("    [cancelled] ollama runner killed (was still generating after timeout).");
    }
}

// ── per-backend test ──────────────────────────────────────────────────────────

fn run_backend_test(
    label: &str,
    client: &OcrClient,
    b64: &str,
    expected: &[SampleEntry],
    timeout_secs: Option<u64>,
) -> (usize, usize) {
    let mut pass = 0usize;
    let mut fail = 0usize;
    let mut _pass = |msg: &str| { println!("  [PASS] {msg}"); pass += 1; };
    let mut _fail = |msg: &str| { println!("  [FAIL] {msg}"); fail += 1; };

    println!("=======================================================");
    println!("==> Backend: {label}");
    println!();

    let t0 = Instant::now();
    let api_result = client.call_api(b64);
    let elapsed_ms = t0.elapsed().as_millis();
    println!("    elapsed : {elapsed_ms} ms  ({:.1}s)", elapsed_ms as f64 / 1000.0);
    println!();

    // ── Check 1: API call succeeded ───────────────────────────────────────────
    println!("--- Check 1: API call succeeded");
    let results = match api_result {
        Ok(r) => { _pass(&format!("call_api returned {} result(s)", r.len())); r }
        Err(e) => {
            let timed_out = timeout_secs
                .map(|n| elapsed_ms >= n.saturating_sub(2) as u128 * 1000)
                .unwrap_or(false);
            if timed_out && label.contains("ollama") {
                cancel_ollama_runner();
                _fail(&format!("timed out after {elapsed_ms} ms (limit: {}s)", timeout_secs.unwrap_or(0)));
                println!("       Inference is too slow — likely running on CPU.");
                println!("       Fix:  scripts/setup.sh --gpu   (requires NVIDIA GPU + CUDA)");
                println!("       Or measure full CPU time: scripts/test-ocr.sh --no-timeout");
            } else if timed_out {
                _fail(&format!("timed out after {elapsed_ms} ms (limit: {}s)", timeout_secs.unwrap_or(0)));
            } else {
                _fail(&format!("call_api failed — {e}"));
            }
            println!();
            println!("  Backend {label}: {pass} passed, {fail} failed  (elapsed: {elapsed_ms} ms)");
            return (pass, fail);
        }
    };

    // ── Check 2: Results are non-empty ────────────────────────────────────────
    println!("--- Check 2: non-empty results");
    if !results.is_empty() {
        _pass(&format!("{} text block(s) returned", results.len()));
    } else {
        _fail("model returned 0 results");
    }

    // ── Check 3: result count ─────────────────────────────────────────────────
    // Fail only if fewer blocks than expected — extra blocks mean the model found
    // more regions, which is fine; missing blocks means something was overlooked.
    println!("--- Check 3: result count");
    match results.len().cmp(&expected.len()) {
        std::cmp::Ordering::Equal => {
            _pass(&format!("{} text blocks (expected {})", results.len(), expected.len()));
        }
        std::cmp::Ordering::Greater => {
            _pass(&format!("{} text blocks returned (expected {}) — extra blocks OK", results.len(), expected.len()));
        }
        std::cmp::Ordering::Less => {
            _fail(&format!("only {} text blocks returned (expected {})", results.len(), expected.len()));
        }
    }

    // ── Check 4: expected text substrings present ─────────────────────────────
    // Coordinate checks are informational WARN only — the expected JSON uses
    // original-image coords (2816×1536) while the model sees a scaled copy.
    println!("--- Check 4: expected text blocks present");
    for entry in expected {
        let text_len = entry.text.chars().count();
        let start = text_len / 4;
        let len = (text_len / 2).max(2);
        let substr: String = entry.text.chars().skip(start).take(len).collect();

        let matched = results.iter().find(|r| r.original.contains(&substr));
        match matched {
            None => {
                _fail(&format!("[{}] text not found — expected substring '{substr}'", entry.label));
                let originals: Vec<&str> = results.iter().map(|r| r.original.as_str()).collect();
                println!("    actual: {originals:?}");
            }
            Some(r) => {
                _pass(&format!("[{}] text found: {}", entry.label, r.original));

                // Coordinates — informational WARN, not a FAIL
                for (field, exp_xy) in [("top_xy", entry.top_xy), ("bot_xy", entry.bot_xy)] {
                    let coord = if field == "top_xy" { r.top_xy.as_deref() } else { r.bot_xy.as_deref() };
                    if let Some(s) = coord {
                        println!(
                            "  [WARN] [{}] {field}: model={s}  json-ref(orig-scale)={},{} \
                             (informational only — coords differ by scale factor)",
                            entry.label, exp_xy[0], exp_xy[1]
                        );
                    }
                }
            }
        }
    }

    println!();
    println!("  Backend {label}: {pass} passed, {fail} failed  (elapsed: {elapsed_ms} ms)");
    (pass, fail)
}

// ── main ──────────────────────────────────────────────────────────────────────

fn main() -> std::process::ExitCode {
    let cfg = parse_args();

    // Load and validate assets
    if !cfg.image_path.exists() {
        eprintln!("ERROR: image not found: {}", cfg.image_path.display());
        return std::process::ExitCode::FAILURE;
    }
    if !cfg.expected_path.exists() {
        eprintln!("ERROR: expected JSON not found: {}", cfg.expected_path.display());
        return std::process::ExitCode::FAILURE;
    }

    let expected: Vec<SampleEntry> = serde_json::from_str(
        &std::fs::read_to_string(&cfg.expected_path)
            .unwrap_or_else(|e| { eprintln!("ERROR reading {}: {e}", cfg.expected_path.display()); std::process::exit(1); })
    ).unwrap_or_else(|e| { eprintln!("ERROR parsing expected JSON: {e}"); std::process::exit(1); });

    // Load + resize image once; share the base64 across backends
    println!("==> Encoding image...");
    let img = image::open(&cfg.image_path)
        .unwrap_or_else(|e| { eprintln!("ERROR opening image: {e}"); std::process::exit(1); });

    let img_resized = if cfg.max_dim > 0 {
        use lenzu::utils; // for dimensions via GenericImageView
        let _ = &utils::encode_as_grayscale; // suppress unused warning
        let (w, h) = (img.width(), img.height());
        if w > cfg.max_dim || h > cfg.max_dim {
            img.resize(cfg.max_dim, cfg.max_dim, FilterType::Lanczos3)
        } else {
            img
        }
    } else {
        img
    };
    println!("    max-dim : {}px  →  {}×{} (grayscale)",
        cfg.max_dim, img_resized.width(), img_resized.height());

    // Both backends receive the same grayscale-encoded resized image.
    // (In production, DualOcrClient handles encoding internally per path;
    //  here we test each client independently with an explicit encoding.)
    let b64 = encode_as_grayscale(&img_resized);
    println!("    b64 size: {} bytes", b64.len());
    println!();

    let timeout_display = match cfg.timeout_secs {
        None    => "none (--no-timeout)".to_string(),
        Some(n) => format!("{n}s"),
    };

    let mut total_pass = 0usize;
    let mut total_fail = 0usize;

    // ── local ollama ──────────────────────────────────────────────────────────

    if !cfg.skip_ollama {
        let client_cfg = AppConfig::default();
        let prompt = client_cfg.resolved_prompt();

        for model in &cfg.ollama_models {
            // Kill any stale runner before each model — prevents VRAM exhaustion.
            cancel_ollama_runner();

            let label = format!("local-ollama:{model}");
            match ollama_preflight(&cfg.ollama_endpoint, model) {
                Err(msg) => {
                    println!("SKIP [{label}] {msg}");
                    total_fail += 1;
                }
                Ok(()) => {
                    println!("    endpoint: {}", cfg.ollama_endpoint);
                    println!("    model   : {model}");
                    println!("    timeout : {timeout_display}");
                    let client = OcrClient::new_with_options(
                        String::new(),
                        cfg.ollama_endpoint.clone(),
                        model.clone(),
                        prompt.clone(),
                        cfg.timeout_secs,
                        cfg.num_ctx,
                        false,  // local ollama: no json_object constraint
                    );
                    let r = run_backend_test(&label, &client, &b64, &expected, cfg.timeout_secs);
                    total_pass += r.0;
                    total_fail += r.1;
                }
            }
        }
    }

    // ── remote OpenRouter ─────────────────────────────────────────────────────

    if !cfg.skip_remote {
        if cfg.remote_key.is_empty() {
            println!("SKIP [remote-openrouter] OPENROUTER_API_KEY not set");
            println!("     Pass --remote-key KEY or export OPENROUTER_API_KEY to enable.");
        } else {
            let client_cfg = AppConfig::default();
            let prompt = client_cfg.resolved_prompt();

            // Remote gets grayscale + downscale (same as DualOcrClient fallback path)
            let remote_b64 = encode_for_fallback(&img_resized, 0); // already sized; no further limit
            println!("    endpoint: {}", cfg.remote_endpoint);
            println!("    model   : {}", cfg.remote_model);
            println!("    timeout : {timeout_display}");
            let client = OcrClient::new_with_options(
                cfg.remote_key.clone(),
                cfg.remote_endpoint.clone(),
                cfg.remote_model.clone(),
                prompt,
                cfg.timeout_secs,
                None,   // num_ctx is ollama-specific; remote backend ignores it
                true,   // remote OpenRouter/Gemini: enable json_object format
            );
            let r = run_backend_test("remote-openrouter", &client, &remote_b64, &expected, cfg.timeout_secs);
            total_pass += r.0;
            total_fail += r.1;
        }
    }

    // ── combined summary ──────────────────────────────────────────────────────

    println!("=======================================================");
    println!("Total: {total_pass} passed, {total_fail} failed");
    if total_fail > 0 {
        std::process::ExitCode::FAILURE
    } else {
        std::process::ExitCode::SUCCESS
    }
}
