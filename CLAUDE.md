# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

SAPIence is a SAPI 5 TTS engine implemented as a Windows COM in-process server (`cdylib` → `sapience.dll`). It registers a SAPI voice token; when an SAPI client speaks, the engine forwards the text as SSML to a running NVDA instance via NVDA's Controller Client. Windows-only (`#![cfg(windows)]`).

## Build / test commands

```
cargo build --release --target <triple>      # x86_64- / i686- / aarch64-pc-windows-msvc
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
cargo test --features test-hooks --target <triple>
cargo test --features test-hooks --test instantiate    # single integration test file
```

`build.rs` downloads `nvda_<NVDA_CONTROLLER_VERSION>_controllerClient.zip` on first build (constant in `build.rs`), extracts the per-arch directory, runs `bindgen` on `nvdaController.h`, emits delay-load link directives, and copies `nvdaControllerClient.dll` next to the cdylib in the profile directory. Offline: set `SAPIENCE_NVDA_CONTROLLER_DIR` to a pre-extracted controller-client root.

Integration tests load the built DLL via `libloading`. Override the path with `SAPIENCE_DLL_PATH`; otherwise `tests/common/mod.rs::dll_path()` resolves `target/<triple>/<profile>/sapience.dll`. CI uses this env var to load the artifact produced by the build job. The `test-hooks` feature gates `sapience_test_set_pid_override` / `clear_pid_override` exports used by `tests/nvda_pid_refusal.rs` to fake NVDA's PID.

## Architecture

- `lib.rs` — DLL entry points: `DllMain` (init tracing, preload `nvdaControllerClient.dll` from the DLL's own directory via `LoadLibraryExW` + altered search path, since the import is delay-loaded), `DllGetClassObject`, `DllCanUnloadNow`, `DllRegisterServer`/`DllUnregisterServer`. `INSTANCE` + `OBJECT_COUNT` are global atomics; `OBJECT_COUNT` drives `DllCanUnloadNow`.
- `class_factory.rs` — `IClassFactory` constructing `TtsEngine` instances.
- `tts_engine.rs` — `#[implement(ISpTTSEngine, ISpObjectWithToken)]`. `Speak` walks `SPVTEXTFRAG` chains via `fragments::iter`, builds SSML in `ssml.rs`, and dispatches to `nvda`.
- `fragments.rs` — iterator over SAPI fragment linked lists; `SPVA_Speak` / `SPVA_Silence` / `SPVA_Bookmark` / `SPVA_SpellOut` action dispatch lives here and in `tts_engine.rs`.
- `ssml.rs` — fragment-to-SSML emission, prosody (rate/volume/pitch) accumulation.
- `pacing.rs` — two pacing loops; `Speak` picks one per call via `wants_timed_events(interest)`. `pace_until_end` writes silent PCM (`SAMPLE_RATE_HZ`, `BYTES_PER_SAMPLE`) so SAPI has an audio-stream offset to fire word/bookmark events against — used only when the client subscribed to timed events. `poll_until_end` writes no PCM and fires no events; used for the common case (default client interest is 0), where the synchronous NVDA worker alone gates `Speak` duration and a silence fragment's gap is just the `cap` wall-clock wait. `marks.rs` handles bookmark/SSML mark plumbing back to `ISpTTSEngineSite` (`wait_drain` lets the poll loop block without spinning).
- `nvda.rs` — safe wrappers over generated `bindgen` bindings (`OUT_DIR/bindings.rs`). Exposes `speak_ssml`, `cancel_speech`, `get_process_id`, `test_if_running`. Has a `test_hooks` submodule (gated `cfg(test)` or `feature = "test-hooks"`) for PID override.
- `clsid.rs` — CLSID constant + voice token metadata (display name, vendor, LCID, age, gender).
- `registry.rs` — writes the COM CLSID entry and the SAPI voice token under a passed-in `Key` (production: `HKEY_LOCAL_MACHINE`; tests redirect HKLM into HKCU via `RegOverridePredefKey`, see `tests/common/mod.rs::HklmOverride`). Uses KTM `Transaction` for atomicity — that's why HKLM is redirected to a regular HKCU subkey rather than an app-key.

## Things to know

- `nvdaControllerClient.dll` is delay-loaded (`/DELAYLOAD` + `delayimp` in `build.rs`) and preloaded explicitly from the DLL's directory in `DllMain`. Don't change the import to non-delay-load without also changing the preload strategy — the DLL is loaded by SAPI clients whose CWD isn't the SAPIence directory.
- Anything called from SAPI runs on a COM-managed thread; lock with `parking_lot::Mutex` (already the convention). Don't add `#[tokio::main]` or other runtimes.
- Logging goes to `%TEMP%\SAPIence.log`, level read once from `HKCU\Software\SAPIence\LogLevel` (then `HKLM` as fallback). Default `WARN`.
- Release profile is size-optimized (`opt-level = "s"`, LTO, single codegen unit) — keep that in mind when adding deps.
