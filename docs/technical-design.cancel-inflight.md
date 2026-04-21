# Cancel In-Flight OCR — Design

Status: **Approved 2026-04-21** — goals, design, and 4 open questions resolved (see "Decisions" section). Implementation can begin.

## Problem

Shift+Click kicks off an OCR pipeline that can take 15+ s on the free-remote tier.
While a request is in flight, subsequent Shift+Clicks are silently dropped
(`main.rs:703` gates on `!s.is_loading`). The user wants rapid iteration:
"I waited too long — give up, capture this *other* region now."

Current mitigations are inadequate:

- Lowering `remote_timeout_secs` only shortens the window; the user still waits the full timeout.
- The worker is `std::thread::spawn` + `reqwest::blocking::Client`. There is no
  handle, no cancel signal, and no way to interrupt a blocking socket read from
  outside the thread.

## Goals

1. A new Shift+Click (or Ctrl+Shift+Click) while an OCR is in-flight **cancels the in-flight request** and starts the new one immediately — no waiting for the old timeout.
2. "Cancel" means the in-flight HTTP socket is closed so the remote backend stops billing/computing, not just "discard the response when it eventually arrives."
3. No stale result ever paints the UI — if an old request completes after a new one has started, its result is dropped on the floor.
4. Latest-wins semantics: only one OCR pipeline is active at a time. We don't fan out parallel requests.

## Non-goals

- Cancel via keyboard (ESC stays on quit for now — can be layered later).
- Parallel/concurrent OCR requests.
- Cancelling local CPU-bound inference (jp_detect + manga-ocr-rs) mid-compute.
  Sub-2 s path — acceptable to let it finish and discard result.
- Cancelling the Electron HUD from the client side.

## Current state (baseline)

- `lenzu/src/main.rs:791` spawns a raw `std::thread::spawn` per Shift+Click with a giant closure.
- `lenzu/src/client.rs:3` uses `reqwest::blocking::Client`; every backend call (local ollama, free remote, paid remote) is a blocking HTTP round-trip.
- Results flow back via `async-channel` into `glib::MainContext::default().spawn_local` (main.rs:536) — this is *not* a tokio runtime; it's glib's executor running on the GTK main thread.
- `is_loading: bool` in AppState (main.rs:119) serves as a mutex: new clicks are rejected while true.
- `tokio` is only a `dev-dependencies` entry (Cargo.toml:43) — no runtime exists in the main binary.

## Proposed design

Two mechanisms in combination:

### A. tokio runtime + async reqwest + `AbortHandle`

- Add `tokio = { version = "1", features = ["rt-multi-thread", "macros", "time", "sync"] }` to `[dependencies]`.
- Create a single `tokio::runtime::Runtime` at app startup; stash its `Handle` in `AppState` (or a `OnceLock<Handle>` global).
- Migrate `OcrClient` from `reqwest::blocking::Client` → `reqwest::Client`. Every `send_and_parse` becomes `async`.
- Sync CPU work (DBNet detect, manga-ocr-rs recognize, grayscale conversion, image crop) is wrapped in `tokio::task::spawn_blocking` so it doesn't stall the runtime.
- Replace the `std::thread::spawn` worker with `runtime.spawn(async move { ... })` returning a `JoinHandle<()>`. `JoinHandle::abort()` closes the underlying TCP socket when called mid-`reqwest`, which is the unlock.
- Store the handle: `in_flight: Option<tokio::task::JoinHandle<()>>` in AppState.
- On new Shift+Click: `if let Some(h) = state.in_flight.take() { h.abort(); }` before spawning the new task.

### B. Generation id for result filtering

`abort()` kills future work on the old task, but previews already sent through
the `async-channel` *before* the abort was called will still be in the buffer
and will still paint the UI. To guarantee no stale frame ever appears:

- `AppState` gets `current_generation: u64`, incremented on every Shift+Click.
- The worker task captures its generation at spawn time and stamps every
  `tx.send(...)` with it: `tx.send((gen_id, Ok(result))).await`.
- The receiver (glib `spawn_local` loop at main.rs:537) compares the incoming
  stamp to `state.current_generation` and drops mismatches silently.

Cheap, airtight. Doesn't replace `abort()` — complements it.

### C. Remove the `is_loading` gate

The existing check at main.rs:703 blocks *stacking*. Under the new semantics
we want stacking — each new click cancels the previous. Remove the
`!s.is_loading` clause from `should_capture`. Keep the 1 s debounce (prevents
accidental double-clicks from firing two captures too close together, which is
a different concern from "user deliberately clicks again because they're tired
of waiting").

### D. Spinner lifecycle

`is_loading` still drives the spinner animation (main.rs:614–625). Keep it
as a visual state, but have it track `in_flight.is_some()` rather than gate
input. When a new request starts, the spinner resets; when it completes or
is aborted, it clears.

## Migration plan (implementation order)

Each step compiles and runs in isolation — no big-bang rewrite.

1. **Add tokio runtime, no behaviour change.** Create `Runtime` at startup,
   stash handle, don't use it yet. Verifies dep graph compiles.

2. **Port `client.rs` from blocking → async.** Big mechanical change: every
   `fn` that calls `self.client.post(...).send()` becomes `async fn` with
   `.await`. All timeout handling moves to `tokio::time::timeout(dur, fut)`.
   `reqwest::blocking::Response::text()` → `reqwest::Response::text().await`.
   SSE streaming (client.rs:350) needs the async equivalent — `reqwest::Response::bytes_stream()` + `futures::StreamExt`.
   Tests use `wiremock` (already a dev-dep) — should port cleanly.

3. **Wrap sync CPU work in `spawn_blocking`.** `jp_detect.detect(...)`,
   `manga-ocr-rs.recognize(...)`, grayscale, crop. Each call site becomes
   `tokio::task::spawn_blocking(move || { ... }).await?`.

4. **Replace `std::thread::spawn` with `runtime.spawn(async move { ... })`.**
   The giant closure in main.rs:791 becomes an async block. `send_blocking`
   on `tx_clone` becomes `tx_clone.send(...).await`.

5. **Add `in_flight: Option<JoinHandle<()>>` + abort-on-new-click.**
   On Shift+Click: take old handle, abort it, spawn new task, store handle.

6. **Add generation id stamp + receiver filter.** `AtomicU64` on AppState;
   each click bumps; worker captures at spawn; every send stamps; receiver
   drops mismatches.

7. **Drop `is_loading` as a gate**, keep it as spinner state.

Order matters: steps 1–4 land without behaviour change (pipeline runs on
tokio but nothing cancels). Steps 5–7 introduce the user-visible behaviour.
This means each commit is reviewable and revertable.

## Testing

- **Unit (wiremock):** set up a mock endpoint with a 10 s delay; start request;
  call `JoinHandle::abort()`; assert the future resolves with
  `JoinError::is_cancelled() == true` in < 100 ms. Proves TCP socket actually
  closes on abort.
- **Unit (generation id):** spawn two requests with gens 1 and 2; gen-1 sends
  a result after gen-2 completes; assert UI receiver drops gen-1.
- **Integration (manual):** Shift+Click, wait 3 s, Shift+Click elsewhere.
  Observe in ollama/OpenRouter logs that the first request was cut. Observe
  the HUD never flashes the first region's result.
- **Regression:** existing OCR accuracy tests run against the new async client
  unchanged (only the wrapper shape changes; the wire protocol doesn't).

## Risks

| Risk | Mitigation |
| --- | --- |
| Async migration touches every call site in client.rs (~1000 lines) | Migrate in one PR; wiremock unit tests catch regressions |
| SSE streaming path is the trickiest — block-reading bytes currently | `reqwest::Response::bytes_stream()` + `tokio::select!` with abort signal; existing wiremock tests cover parse logic |
| `spawn_blocking` for local OCR means abort doesn't kill CPU inference mid-flight | Accepted per non-goal #3. Local OCR is ~0.8–2 s; user pressing Shift+Click during that window waits ≤ 2 s max |
| Increased binary size from `tokio` + `tokio-rt-multi-thread` | ~1–2 MB added — negligible for a desktop app |
| Runtime lifetime on app shutdown: in-flight tasks may outlive GTK `main_quit()` | `Runtime::shutdown_timeout(Duration::from_secs(2))` in the ESC/quit path |
| Panic in async task ≠ panic in thread — existing `catch_unwind` wrapper needs re-review | `tokio::task::JoinHandle::await` returns `Result<_, JoinError>`; `JoinError::is_panic()` is observable. Worker wraps its body in `AssertUnwindSafe` / tokio equivalent |

## Decisions (2026-04-21)

1. **Cancel trigger scope: any shift-modified user command.** Not just Shift+Click and Ctrl+Shift+Click. Shift+Tab (toggle direction), Shift+H (help dialog), and a new Shift+ESC (explicit cancel) also cancel in-flight OCR. All share the same `in_flight` slot. Rationale: a shift-modified action is always deliberate user input; treat it uniformly as "user has moved on, drop pending work."

2. **Cancelled request logging: `[cancelled]` line in debug log, no history entry.** Confirmed.

3. **Plain ESC stays on quit.** Non-shift inputs do not cancel. New Shift+ESC binding is added for explicit cancel-without-quit. Plain ESC continues to call `kill_server` + `gtk::main_quit()` (main.rs:401) unchanged.

4. **Runtime ownership: owned by `main`, `Handle` cloned into AppState.** No static state, no `OnceLock`. Clone the handle wherever it's needed; `tokio::runtime::Handle` is explicitly cheap to clone and clone-safe. Runtime is dropped on app exit, flushing in-flight work via `Runtime::shutdown_timeout(Duration::from_secs(2))`.

### Implications for step 5 (abort-on-new-click)

The abort trigger is no longer just Shift+Click / Ctrl+Shift+Click. Factor the
cancel logic into a single helper on AppState:

```rust
impl AppState {
    fn cancel_inflight(&mut self, reason: &'static str) {
        if let Some(h) = self.in_flight.take() {
            eprintln!("[OCR] cancel: {reason}");
            h.abort();
        }
        self.current_generation = self.current_generation.wrapping_add(1);
    }
}
```

Call sites:
- `Shift+Click` / `Ctrl+Shift+Click` — `cancel_inflight("new-capture")` then spawn.
- `Shift+Tab` (toggle direction, main.rs:416) — `cancel_inflight("direction-toggle")`.
- `Shift+H` (help dialog, main.rs:408) — `cancel_inflight("help-dialog")`.
- `Shift+ESC` (new) — `cancel_inflight("user-cancel")`, no quit.
- Plain `ESC` — unchanged; quit path already kills the runtime via shutdown.

## Estimated effort

- Design doc + review: **done when this file is approved**.
- Step 1 (tokio dep + runtime): ~30 min.
- Step 2 (client.rs blocking → async): **largest chunk — 3–5 h.** Most of the PR.
- Steps 3–4 (worker to async): 1–2 h.
- Steps 5–6 (abort + generation id): 1 h.
- Step 7 (is_loading cleanup): 15 min.
- Test pass + manual verification: 1 h.

Total: ~1 working day for a clean implementation with tests.

## Links

- Current worker: `lenzu/src/main.rs:697–1237`
- Current client: `lenzu/src/client.rs` (entire file)
- Relevant config: `lenzu/src/config.rs:120–133` (timeout fields)
- Related: `docs/technical-design.md` — overall architecture
