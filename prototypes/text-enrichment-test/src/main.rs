use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::BufRead;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct EnrichmentResult {
    original: Option<String>,
    furigana: Option<String>,
    romaji: Option<String>,
    english: Option<String>,
}

fn read_sse_content(res: reqwest::blocking::Response) -> Result<String> {
    let reader = std::io::BufReader::new(res);
    let mut content = String::new();
    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed == "data: [DONE]" {
            continue;
        }
        if let Some(json_part) = trimmed.strip_prefix("data: ") {
            if let Ok(chunk) = serde_json::from_str::<Value>(json_part) {
                if let Some(delta) = chunk["choices"][0]["delta"]["content"].as_str() {
                    content.push_str(delta);
                }
            }
        }
    }
    Ok(content)
}

fn strip_markdown_fences(raw: &str) -> &str {
    let s = raw.trim();
    if s.starts_with("```") {
        if let Some(newline) = s.find('\n') {
            let inner = s[newline + 1..].trim_end();
            if inner.ends_with("```") {
                return inner[..inner.len() - 3].trim_end();
            }
            if let Some(end) = inner.rfind("\n```") {
                return inner[..end].trim();
            }
        }
    }
    s
}

fn enrich(
    client: &Client,
    endpoint: &str,
    model: &str,
    prompt: &str,
    text: &str,
    timeout_secs: u64,
) -> Result<EnrichmentResult> {
    let payload = json!({
        "model": model,
        "messages": [{
            "role": "user",
            "content": format!("{prompt}\n\nText: {text}")
        }],
        "temperature": 0.1,
        "stream": true
    });

    let res = client
        .post(endpoint)
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .json(&payload)
        .send()
        .context("failed to connect to Ollama")?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().unwrap_or_default();
        bail!("Ollama HTTP {status}: {}", body.chars().take(200).collect::<String>());
    }

    let content = read_sse_content(res)?;
    let clean = strip_markdown_fences(&content);
    let result: EnrichmentResult =
        serde_json::from_str(clean).context(format!("bad JSON from LLM: {}", clean.chars().take(300).collect::<String>()))?;
    Ok(result)
}

struct TestCase {
    input: &'static str,
    furigana_contains: &'static str,
    english_contains: &'static str,
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    let mut model = "glm-ocr".to_string();
    let mut timeout: u64 = 15;
    let mut endpoint = "http://localhost:11434/v1/chat/completions".to_string();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--model" => { i += 1; model = args[i].clone(); }
            "--timeout" => { i += 1; timeout = args[i].parse()?; }
            "--endpoint" => { i += 1; endpoint = args[i].clone(); }
            other => bail!("unknown arg: {other}"),
        }
        i += 1;
    }

    // Preflight: check Ollama is running
    let client = Client::new();
    let health = client
        .get(endpoint.replace("/v1/chat/completions", "/api/tags"))
        .timeout(std::time::Duration::from_secs(3))
        .send();
    match health {
        Err(e) => bail!("Ollama not reachable at {endpoint}: {e}"),
        Ok(r) if !r.status().is_success() => bail!("Ollama health check failed: {}", r.status()),
        _ => eprintln!("[ok] Ollama is running"),
    }

    let prompt = "You are a Japanese-to-English language expert. Given the following text, return a JSON object with: \
        'original' (the exact input text), \
        'furigana' (format: 漢字[かんじ]), \
        'romaji' (romanization), \
        'english' (translation to English), \
        'debug_info' (null). \
        Return ONLY the JSON object.";

    let cases = vec![
        TestCase {
            input: "食べる",
            furigana_contains: "た",
            english_contains: "eat",
        },
        TestCase {
            input: "日本語",
            furigana_contains: "にほんご",
            english_contains: "",  // too variable across models
        },
        TestCase {
            input: "漢字",
            furigana_contains: "かんじ",
            english_contains: "",
        },
        TestCase {
            input: "最近人気のデスクトップなリナックスです!",
            furigana_contains: "さいきん",
            english_contains: "",
        },
    ];

    eprintln!("model: {model}, timeout: {timeout}s, endpoint: {endpoint}\n");

    let mut passed = 0;
    let mut failed = 0;

    for (i, tc) in cases.iter().enumerate() {
        eprint!("[{}/{}] {:30} → ", i + 1, cases.len(), tc.input);
        let t0 = std::time::Instant::now();
        match enrich(&client, &endpoint, &model, prompt, tc.input, timeout) {
            Ok(result) => {
                let elapsed = t0.elapsed();
                let furigana = result.furigana.as_deref().unwrap_or("(none)");
                let english = result.english.as_deref().unwrap_or("(none)");
                eprintln!("furigana={furigana}  english={english}  ({:.1}s)", elapsed.as_secs_f64());

                let mut ok = true;
                if !tc.furigana_contains.is_empty() && !furigana.contains(tc.furigana_contains) {
                    eprintln!("  FAIL: furigana missing {:?}", tc.furigana_contains);
                    ok = false;
                }
                if !tc.english_contains.is_empty()
                    && !english.to_lowercase().contains(&tc.english_contains.to_lowercase())
                {
                    eprintln!("  FAIL: english missing {:?}", tc.english_contains);
                    ok = false;
                }
                if ok {
                    passed += 1;
                } else {
                    failed += 1;
                }
            }
            Err(e) => {
                eprintln!("ERROR: {e}");
                failed += 1;
            }
        }
    }

    eprintln!("\n{passed}/{} passed, {failed} failed", passed + failed);
    if failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}
