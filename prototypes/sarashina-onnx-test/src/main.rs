// Sarashina 2.2 0.5b-instruct ONNX prototype.
//
// Milestones (docs/technical-design.sarashina.md §4.4):
//   M1 — tokenizer encode/decode round-trip
//   M2 — single forward pass, dump logits shape + argmax next token
//   M3 — greedy generation with KV cache
//   bench — time a generation run
//
// Usage:
//   sarashina-onnx-test <model_dir> tok      <text>
//   sarashina-onnx-test <model_dir> forward  <text>
//   sarashina-onnx-test <model_dir> gen      <prompt> [max_new]
//   sarashina-onnx-test <model_dir> bench    <prompt> [max_new]

use anyhow::{anyhow, bail, Context, Result};
use half::f16;
use ort::memory::Allocator;
use ort::session::Session;
use ort::value::{DynValue, Tensor, Value};
use std::path::{Path, PathBuf};
use std::time::Instant;
use tokenizers::Tokenizer;

#[derive(Debug, serde::Deserialize)]
struct Config {
    num_hidden_layers: usize,
    num_key_value_heads: usize,
    head_dim: usize,
    eos_token_id: u32,
    bos_token_id: u32,
    vocab_size: usize,
    dtype: String, // "float16" | "float32"
}

impl Config {
    fn is_fp16(&self) -> bool {
        self.dtype == "float16"
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        print_usage(&args[0]);
        std::process::exit(2);
    }
    let model_dir = PathBuf::from(&args[1]);
    let cmd = args[2].as_str();

    let config = load_config(&model_dir).context("load config.json")?;
    let tok = Tokenizer::from_file(model_dir.join("tokenizer.json"))
        .map_err(|e| anyhow!("load tokenizer.json: {e}"))?;

    println!(
        "model: {} layers, {} kv-heads, head_dim={}, vocab={}, dtype={}",
        config.num_hidden_layers,
        config.num_key_value_heads,
        config.head_dim,
        config.vocab_size,
        config.dtype
    );

    match cmd {
        "tok" => {
            let text = args.get(3).context("usage: tok <text>")?;
            m1_tokenize(&tok, text)
        }
        "forward" => {
            let text = args.get(3).context("usage: forward <text>")?;
            m2_forward(&model_dir, &config, &tok, text)
        }
        "gen" => {
            let prompt = args.get(3).context("usage: gen <prompt> [max_new]")?;
            let max_new = args.get(4).map(|s| s.parse()).transpose()?.unwrap_or(64);
            m3_generate(&model_dir, &config, &tok, prompt, max_new, false)
        }
        "bench" => {
            let prompt = args.get(3).context("usage: bench <prompt> [max_new]")?;
            let max_new = args.get(4).map(|s| s.parse()).transpose()?.unwrap_or(64);
            m3_generate(&model_dir, &config, &tok, prompt, max_new, true)
        }
        _ => bail!("unknown command: {cmd}"),
    }
}

fn print_usage(prog: &str) {
    eprintln!("usage: {prog} <model_dir> <cmd> [args]");
    eprintln!("commands:");
    eprintln!("  tok <text>            M1: tokenize round-trip");
    eprintln!("  forward <text>        M2: one forward pass, dump shapes");
    eprintln!("  gen <prompt> [N]      M3: greedy generation (not impl yet)");
    eprintln!("  bench <prompt> [N]    bench generation (not impl yet)");
}

fn load_config(model_dir: &Path) -> Result<Config> {
    let text = std::fs::read_to_string(model_dir.join("config.json"))?;
    Ok(serde_json::from_str(&text)?)
}

// --- M1 -----------------------------------------------------------------

fn m1_tokenize(tok: &Tokenizer, text: &str) -> Result<()> {
    let enc = tok
        .encode(text, false)
        .map_err(|e| anyhow!("encode: {e}"))?;
    let ids = enc.get_ids();

    println!("\nencoded {} tokens:", ids.len());
    for &id in ids {
        let piece = tok.id_to_token(id).unwrap_or_else(|| "?".into());
        println!("  {:>6}  {}", id, piece);
    }

    let decoded = tok
        .decode(ids, false)
        .map_err(|e| anyhow!("decode: {e}"))?;
    println!("\ndecoded: {decoded:?}");

    let input_trim = text.trim();
    let decoded_trim = decoded.trim();
    if input_trim == decoded_trim {
        println!("round-trip: OK (exact)");
    } else if decoded_trim.contains(input_trim) {
        println!("round-trip: OK (decoded contains input; whitespace/special-token drift)");
    } else {
        println!("round-trip: DIFFERS");
        println!("  input:   {input_trim:?}");
        println!("  decoded: {decoded_trim:?}");
    }
    Ok(())
}

// --- M2 -----------------------------------------------------------------

fn m2_forward(model_dir: &Path, config: &Config, tok: &Tokenizer, text: &str) -> Result<()> {
    let t0 = Instant::now();
    let mut session = Session::builder()?.commit_from_file(model_dir.join("model.onnx"))?;
    println!("session load: {:.2?}", t0.elapsed());

    println!("\n=== session I/O ===");
    for inp in session.inputs() {
        println!("  in : {:30} {:?}", inp.name(), inp.dtype());
    }
    for out in session.outputs() {
        println!("  out: {:30} {:?}", out.name(), out.dtype());
    }

    let enc = tok
        .encode(text, true)
        .map_err(|e| anyhow!("encode: {e}"))?;
    let ids: Vec<i64> = enc.get_ids().iter().map(|&x| x as i64).collect();
    let n = ids.len();
    println!("\nprompt tokens: {n}");

    let inputs = build_inputs(config, session.allocator(), &ids)?;

    let t0 = Instant::now();
    let outputs = session.run(inputs)?;
    println!("forward pass: {:.2?}", t0.elapsed());

    let logits = &outputs["logits"];
    let (shape, best_id, best_logit) = if config.is_fp16() {
        let (shape, data) = logits.try_extract_tensor::<f16>()?;
        let (id, val) = argmax_last(&data, &shape, |x| x.to_f32());
        (shape, id, val)
    } else {
        let (shape, data) = logits.try_extract_tensor::<f32>()?;
        let (id, val) = argmax_last(&data, &shape, |x| *x);
        (shape, id, val)
    };

    let piece = tok.id_to_token(best_id as u32).unwrap_or_else(|| "?".into());
    println!("\nlogits shape: {:?}", shape);
    println!("argmax next token: id={best_id} ({piece:?}) logit={best_logit:.3}");

    Ok(())
}

fn build_inputs<'a>(
    config: &Config,
    allocator: &Allocator,
    ids: &[i64],
) -> Result<Vec<(std::borrow::Cow<'a, str>, DynValue)>> {
    let n = ids.len();
    let input_ids_val = Value::from_array(([1usize, n], ids.to_vec().into_boxed_slice()))?.into_dyn();
    let attn_val = Value::from_array(([1usize, n], vec![1i64; n].into_boxed_slice()))?.into_dyn();
    let pos_val = Value::from_array((
        [1usize, n],
        (0..n as i64).collect::<Vec<_>>().into_boxed_slice(),
    ))?
    .into_dyn();

    let mut inputs: Vec<(std::borrow::Cow<'a, str>, DynValue)> = vec![
        ("input_ids".into(), input_ids_val),
        ("attention_mask".into(), attn_val),
        ("position_ids".into(), pos_val),
    ];

    let kv_shape = [1i64, config.num_key_value_heads as i64, 0, config.head_dim as i64];
    for i in 0..config.num_hidden_layers {
        let (k, v) = if config.is_fp16() {
            (
                Tensor::<f16>::new(allocator, kv_shape)?.into_dyn(),
                Tensor::<f16>::new(allocator, kv_shape)?.into_dyn(),
            )
        } else {
            (
                Tensor::<f32>::new(allocator, kv_shape)?.into_dyn(),
                Tensor::<f32>::new(allocator, kv_shape)?.into_dyn(),
            )
        };
        inputs.push((format!("past_key_values.{i}.key").into(), k));
        inputs.push((format!("past_key_values.{i}.value").into(), v));
    }
    Ok(inputs)
}

fn argmax_last<T: Copy>(
    data: &[T],
    shape: &[i64],
    to_f32: impl Fn(&T) -> f32,
) -> (usize, f32) {
    let seq = shape[1] as usize;
    let vocab = shape[2] as usize;
    let start = (seq - 1) * vocab;
    let last_row = &data[start..start + vocab];
    let mut best_id = 0usize;
    let mut best_val = f32::NEG_INFINITY;
    for (i, v) in last_row.iter().enumerate() {
        let f = to_f32(v);
        if f > best_val {
            best_val = f;
            best_id = i;
        }
    }
    (best_id, best_val)
}

// --- M3 -----------------------------------------------------------------

fn m3_generate(
    model_dir: &Path,
    config: &Config,
    tok: &Tokenizer,
    prompt: &str,
    max_new: usize,
    bench: bool,
) -> Result<()> {
    let t_load = Instant::now();
    let mut session = Session::builder()?.commit_from_file(model_dir.join("model.onnx"))?;
    let load_elapsed = t_load.elapsed();

    let enc = tok.encode(prompt, true).map_err(|e| anyhow!("encode: {e}"))?;
    let prompt_ids: Vec<i64> = enc.get_ids().iter().map(|&x| x as i64).collect();
    let n_prompt = prompt_ids.len();
    println!("prompt: {n_prompt} tokens");

    let argmax_logits = |logits: &DynValue| -> Result<u32> {
        let id = if config.is_fp16() {
            let (shape, data) = logits.try_extract_tensor::<f16>()?;
            argmax_last(&data, &shape, |x| x.to_f32()).0
        } else {
            let (shape, data) = logits.try_extract_tensor::<f32>()?;
            argmax_last(&data, &shape, |x| *x).0
        };
        Ok(id as u32)
    };

    let mut generated: Vec<u32> = Vec::new();
    let prefill_inputs = build_inputs(config, session.allocator(), &prompt_ids)?;
    let mut past_len: usize = 0;

    let t_prefill = Instant::now();
    let mut outputs = session.run(prefill_inputs)?;
    let prefill_elapsed = t_prefill.elapsed();
    past_len += n_prompt;

    let mut next_id = argmax_logits(&outputs["logits"])?;
    if next_id != config.eos_token_id {
        generated.push(next_id);
    }

    let t_decode = Instant::now();
    let mut decode_steps = 0usize;
    while next_id != config.eos_token_id && generated.len() < max_new {
        let step_inputs = build_step_inputs(config, &mut outputs, next_id, past_len)?;
        drop(outputs);
        past_len += 1;
        outputs = session.run(step_inputs)?;
        next_id = argmax_logits(&outputs["logits"])?;
        if next_id == config.eos_token_id {
            break;
        }
        generated.push(next_id);
        decode_steps += 1;
    }
    let decode_elapsed = t_decode.elapsed();

    let text = tok
        .decode(&generated, true)
        .map_err(|e| anyhow!("decode: {e}"))?;

    println!("\n=== generated ===\n{text}\n=================");
    println!(
        "tokens generated: {} (prefill={}, decode_steps={})",
        generated.len(),
        n_prompt,
        decode_steps
    );

    if bench {
        let decode_s = decode_elapsed.as_secs_f64();
        let tps = if decode_steps > 0 && decode_s > 0.0 {
            decode_steps as f64 / decode_s
        } else {
            0.0
        };
        println!("\n=== bench ===");
        println!("  session load  : {load_elapsed:.2?}");
        println!("  prefill ({n_prompt} tok): {prefill_elapsed:.2?}");
        println!("  decode ({decode_steps} tok) : {decode_elapsed:.2?}  ({tps:.2} tok/s)");
    }
    Ok(())
}

fn build_step_inputs<'a>(
    config: &Config,
    prev: &mut ort::session::SessionOutputs<'_>,
    next_id: u32,
    past_len: usize,
) -> Result<Vec<(std::borrow::Cow<'a, str>, DynValue)>> {
    let input_ids_val =
        Value::from_array(([1usize, 1], Box::new([next_id as i64]) as Box<[i64]>))?.into_dyn();
    let attn: Vec<i64> = vec![1; past_len + 1];
    let attn_val = Value::from_array(([1usize, past_len + 1], attn.into_boxed_slice()))?.into_dyn();
    let pos_val =
        Value::from_array(([1usize, 1], Box::new([past_len as i64]) as Box<[i64]>))?.into_dyn();

    let mut inputs: Vec<(std::borrow::Cow<'a, str>, DynValue)> = vec![
        ("input_ids".into(), input_ids_val),
        ("attention_mask".into(), attn_val),
        ("position_ids".into(), pos_val),
    ];
    for i in 0..config.num_hidden_layers {
        let k = prev
            .remove(format!("present.{i}.key"))
            .ok_or_else(|| anyhow!("missing present.{i}.key in outputs"))?;
        let v = prev
            .remove(format!("present.{i}.value"))
            .ok_or_else(|| anyhow!("missing present.{i}.value in outputs"))?;
        inputs.push((format!("past_key_values.{i}.key").into(), k));
        inputs.push((format!("past_key_values.{i}.value").into(), v));
    }
    Ok(inputs)
}
