# sarashina-vision-py — negative result

A Python sidecar prototype to evaluate whether `sbintuitions/sarashina2.2-vision-3b`
can serve as Lenzu's local-fallback OCR tier (replacing the current remote-LLM fallback,
which takes ~60 s per crop).

**Verdict: unusable for Lenzu.** CPU-only inference is orders of magnitude outside the
budget, and the resource cost while running makes the host unusable for anything else.
Quality is unknown — we never got a single completed output.

## Host used for the test

- CPU: 32 logical cores (Linux 6.12)
- RAM: 31.9 GiB
- Swap: 32 GiB
- GPU: Quadro M4000 (sm_5.2, Maxwell) — **not** used; unsupported by modern PyTorch CUDA kernels.
- Device passed to the model: `cpu`, `torch.float32`

## Software stack

After a long dependency-resolution fight (huggingface-hub 1.x's strict dataclass rejects
the model's `mlp_ratio=3.7362` float, and transformers 5.x dropped `AutoModelForVision2Seq`
while the model's `trust_remote_code` loader expects 4.x-era APIs), the working pin is:

- Python 3.13
- `torch` (CPU wheel), `torchvision` (CPU wheel) from `download.pytorch.org/whl/cpu`
- `transformers == 4.49.0`
- `huggingface-hub == 0.28.1`
- `tokenizers == 0.21.0`
- Loaded via `AutoModelForCausalLM.from_pretrained(..., trust_remote_code=True)`
  (the custom `Sarashina2VisionConfig` is only registered under `AutoModelForCausalLM`,
  not `AutoModelForVision2Seq`).

Weights cached on disk: **7.1 GiB**.

## The run

Command:

```
.venv/bin/python test_vision.py --device cpu
```

Battery: 4 images from `assets/` (`Unit-test-tategaki.png`, `Unit-test-yokogaki.png`,
`Unit-test-sample-texts.png`, `ubunchu01_02.png`), each prompted with
`画像の日本語テキストを読んで、英語に翻訳してください。`, `max_new_tokens=256`,
`do_sample=False`.

**Wall time before we gave up and SIGTERM'd it: 2 h 34 min 31 s (9271 s).**

In that time:
- `model.generate()` was invoked 4 times (one per image — the script's for-loop got
  through all four calls), inferred from 4 occurrences of
  `Setting pad_token_id to eos_token_id:2 for open-end generation` in stderr.
- **Zero jobs produced visible output.** The script prints `=== label: file ===` and
  `latency: Xs` / `output: ...` on stdout, but Python block-buffers stdout when piped
  (we did not use `-u`), so the buffered prints were lost when the process was SIGTERM'd.
  The warnings above leaked through because they come from transformers via `warnings` /
  `logging` which flush more aggressively.
- Lower bound on per-job latency: if all four `generate()` calls did in fact each
  contribute roughly proportionally, that's **~38 min per job** on CPU. Even the
  most optimistic reading (call-4 was barely started, jobs 1–3 averaged ~50 min each)
  still puts per-job latency in the tens of minutes.

## Resource usage while running

Sampled throughout the 154 min run via `top`:

| Metric                       | Value                        |
| ---------------------------- | ---------------------------- |
| Process `%CPU`               | **1019 – 1220 %** (sustained) |
| Cores equivalent             | ≈ 10 – 12 out of 32          |
| Process RSS                  | **8.6 – 9.6 GiB**            |
| Process `%MEM` of 32 GiB box | 27 – 30 %                    |
| System load avg (1 / 5 / 15) | 12 – 13 across the run       |
| Total system RAM in use      | 23.4 GiB of 31.9 GiB         |
| Swap in use                  | ≈ 16.7 GiB (some from other processes) |

Translation: one instance of this model, on CPU, eats about a third of a 32-GiB box's
RAM and 10+ cores of CPU, indefinitely, for a single OCR request. The rest of the
machine becomes sluggish. This is not a tier Lenzu can ship as a "local fallback"
on user hardware.

## How it fails Lenzu's budget

Lenzu's OCR tiers (current production):

- Primary (`manga-ocr-rs` DBNet + ONNX recognizer): ~3–5 s per crop on CPU.
- Fallback (remote LLM): ~60 s per crop, acceptable but expensive.

The goal of this prototype was to find a **local** fallback at <15 s per crop.
Sarashina vision on CPU misses by roughly **150×**, and consumes resources that would
make the primary tier unusable in parallel.

## Why we still ran it

Previous discussion had already flagged local VLMs for OCR as "probably unacceptable
on CPU." This run was to get a concrete, reproducible data point rather than dismiss
the option on priors — empirical, not theoretical.

Data point collected. Direction for Lenzu: **skip all local-host VLMs for OCR.**
Dedicated sub-second JP OCR models (manga-ocr-rs and similar) remain the only local
path that fits the latency budget.

## What's here

- `test_vision.py` — the test harness (image → chat template → `generate` → decode).
- `requirements.txt` — pinned working deps.
- `run.log` — the run's stdout+stderr (mostly buffered away, kept for completeness).
- `.venv/lib/python3.13/site-packages/_ipv4_only.pth` — a venv-local `.pth` that forces
  `socket.getaddrinfo` to return only IPv4 addresses, to route around pip's IPv6
  flakiness on `files.pythonhosted.org`. Retained because pip itself is still fragile
  on this host over IPv6.

## Status

**Done.** No further work on this prototype unless we become confident yomitoku can
be made to work in pure Rust (no Python sidecar). Same bar as before: sub-second-per-crop
JP OCR on CPU, small RAM footprint. If that confidence is not there, there is no next
step here — we stop, keep the remote-LLM fallback, and look elsewhere.

## Do-not-repeat notes

- Pass `python -u` or set `PYTHONUNBUFFERED=1` if you ever run this again — without it,
  per-job prints are lost on SIGTERM, which was the single biggest observability miss
  of this run. (With it, we would at least have seen latency and output for jobs 1–3
  before deciding to kill.)
- Do not pair transformers ≥ 5 with this model; `AutoModelForVision2Seq` was renamed
  and `huggingface-hub` ≥ 1.0 rejects the config.
- Do not try this on GPUs older than sm_6.0 — modern PyTorch/ort kernels refuse.
