# SAPIence Design

A Rust SAPI 5 TTS engine that forwards speech to NVDA instead of synthesising audio. Lets SAPI-only hosts (e.g., Komplete Kontrol) speak through the user's screen reader.

## Goals

- In-process COM SAPI 5 TTS engine DLL.
- Forwards all speech to NVDA via the NVDA Controller Client (SSML).
- Honours SAPI prosody (rate, pitch, volume) by mapping to SSML `<prosody>` per fragment.
- Prevents loops: refuses to instantiate when called from NVDA's own process.
- Targets x86, x64, aarch64 Windows.
- License: LGPL-2.1-or-later.

## Non-goals

- No real audio output. Engine writes zero-filled PCM only to satisfy SAPI's `ISpTTSEngineSite::Write` flow-control contract.
- No support for SAPI baseline rate/volume properties on the voice (`ISpVoice::SetRate`/`SetVolume`). Only per-fragment prosody adjustments are forwarded.
- No phoneme synthesis (`SPVA_Pronounce` falls back to plain text).
- No custom SAPI XML tag handling (`SPVA_ParseUnknownTag` is ignored).
- No ARM64EC or ARM64X variants.

## Architecture

```
SAPIence/
├── Cargo.toml          # cdylib, edition 2024, LGPL-2.1-or-later
├── build.rs            # download/cache NVDA controller client + bindgen
├── src/
│   ├── lib.rs          # DllMain, DllGetClassObject, DllCanUnloadNow,
│   │                   # DllRegisterServer, DllUnregisterServer
│   ├── class_factory.rs # IClassFactory + NVDA-PID refusal
│   ├── tts_engine.rs   # ISpTTSEngine + ISpObjectWithToken impl
│   ├── ssml.rs         # SPVTEXTFRAG → SSML, prosody mapping, mark naming
│   ├── pacing.rs       # silent PCM writer, action polling, event scheduling
│   ├── marks.rs        # global mark-callback dispatcher
│   ├── nvda.rs         # safe Rust wrappers around vendored controller bindings
│   └── registry.rs     # COM CLSID + SAPI voice token reg/unreg
├── tests/
│   ├── common/mod.rs   # ported scaffolding from rd_pipe-rs
│   ├── register_unregister.rs
│   ├── instantiate.rs
│   ├── nvda_pid_refusal.rs
│   └── pacing_silence.rs
└── docs/superpowers/specs/2026-05-26-sapience-design.md
```

Dependencies:

- `windows` 0.62 with features: `Win32_Foundation`, `Win32_Media_Speech`, `Win32_System_Com`, `Win32_System_Com_StructuredStorage`, `Win32_System_LibraryLoader`, `Win32_System_Threading`, `Win32_System_Ole`.
- `windows-registry` 0.6.
- NVDA Controller Client: vendored via `build.rs`. Downloads `https://download.nvaccess.org/releases/<version>/nvda_<version>_controllerClient.zip` (both path segments use the same version), caches under `OUT_DIR/nvda-controller-client/<version>/`, runs `bindgen` against `<arch>/nvdaController.h`, links `nvdaControllerClient.lib` from the matching arch dir. The version is a `const` in `build.rs`, bumped manually. Env var `SAPIENCE_NVDA_CONTROLLER_DIR` overrides with a local pre-extracted directory (offline/dev builds; e.g., `P:\A11y\nvda\extras\controllerClient`). The previous `nvda` example crate is no longer a dependency — bindings live in SAPIence's own `nvda::` module.
- `parking_lot`, `tracing`, `tracing-subscriber`, `tracing-appender`.

No bindgen for SAPI — the `windows` crate already exposes `ISpTTSEngine`, `ISpObjectWithToken`, `ISpTTSEngineSite`, `SPVTEXTFRAG`, `SPVSTATE`, `SPEVENT`, etc., under `Win32::Media::Speech`. Bindgen is used only for `nvdaController.h` inside SAPIence's own `build.rs`.

Build-time dependencies (`[build-dependencies]`):

- `bindgen` — generate `nvdaController.h` bindings.
- `reqwest` (blocking, `rustls-tls`) — download the controller client zip.
- `zip` — extract the zip into the build cache.

## Build script

`build.rs` vendors the NVDA Controller Client at build time.

```text
const NVDA_CONTROLLER_VERSION: &str = "2024.4.1";  // bumped manually
const RELEASE_URL_FMT: &str =
    "https://download.nvaccess.org/releases/{ver}/nvda_{ver}_controllerClient.zip";

fn main():
    let arch = match CARGO_CFG_TARGET_ARCH:
        "x86_64"  => "x64"
        "aarch64" => "arm64"
        "x86"     => "x86"
        _ => fail

    // 1. Resolve controller-client root:
    let root = match env::SAPIENCE_NVDA_CONTROLLER_DIR:
        Some(p) => PathBuf::from(p)        // dev/offline override
        None    => download_and_extract(NVDA_CONTROLLER_VERSION,
                                        cache_dir())

    // 2. Per-arch dir contains nvdaController.h and nvdaControllerClient.lib
    let arch_dir = root.join(arch).canonicalize()?

    // 3. Tell cargo where to find the import library:
    println!("cargo:rustc-link-search=native={}", arch_dir.display())
    println!("cargo:rustc-link-lib=nvdaControllerClient")

    // 4. Generate bindings into OUT_DIR/bindings.rs
    bindgen::Builder::default()
        .header(arch_dir.join("nvdaController.h").to_str()?)
        .allowlist_function("nvdaController_.+")
        .prepend_enum_name(false)
        .must_use_type("error_status_t")
        .generate()?
        .write_to_file(OUT_DIR.join("bindings.rs"))?

    // 5. Copy nvdaControllerClient.dll into the cargo output dir so it
    //    ends up next to sapience.dll for packaging/deployment.
    fs::copy(arch_dir.join("nvdaControllerClient.dll"),
             target_profile_dir().join("nvdaControllerClient.dll"))?

    println!("cargo:rerun-if-env-changed=SAPIENCE_NVDA_CONTROLLER_DIR")
    println!("cargo:rerun-if-changed=build.rs")
```

Zip layout (verified against `nvda_2026.1.1_controllerClient.zip`):

```
<root>/
├── license.txt
├── readme.md
├── x86/    nvdaController.h, nvdaControllerClient.{dll,lib,exp,pdb}
├── x64/    "
├── arm64/  "
├── arm64ec/ (ignored)
└── examples/ (ignored)
```

### Runtime DLL loading

`nvdaControllerClient.dll` is a runtime dependency, not a static import. Two problems:

1. **Search path**: when a SAPI host loads `sapience.dll`, Windows resolves `sapience.dll`'s imports against the *host*'s search path, not `sapience.dll`'s directory. The controller client won't be found.
2. **Eager resolution**: standard imports are resolved before `DllMain` runs, so we can't fix the path first.

Solution: **delay-load** `nvdaControllerClient.dll` plus an explicit load from `sapience.dll`'s own directory in `DllMain`.

- Linker flag (MSVC): `/DELAYLOAD:nvdaControllerClient.dll`. Emitted from `build.rs` via `println!("cargo:rustc-link-arg=/DELAYLOAD:nvdaControllerClient.dll");` and `println!("cargo:rustc-link-lib=delayimp");`.
- In `DllMain(DLL_PROCESS_ATTACH)`:
  - `GetModuleFileNameW(INSTANCE, ...)` → strip filename → append `nvdaControllerClient.dll` → `LoadLibraryExW(path, NULL, LOAD_WITH_ALTERED_SEARCH_PATH)`.
  - If load fails, log error but continue — first NVDA API call will surface the error to `Speak` and we return `S_OK` silently.
- Once loaded, Windows uses the already-loaded module for subsequent symbol resolution from the delay-load thunks.

### Deployment

Registration must place `nvdaControllerClient.dll` alongside `sapience.dll`. The `build.rs` copies it into the cargo target/profile dir during build, so the artifacts are co-located there. The release packaging step (out of scope for this design) installs both DLLs together. `DllRegisterServer` writes the registry; it does not move files.

Cache layout: `OUT_DIR/nvda-controller-client/<version>/` (per target, per profile — cheap because the zip is small). If the version dir already exists, skip download. The zip is only fetched on a clean build or version bump.

`src/nvda.rs` includes the generated bindings and exposes safe wrappers (`test_if_running`, `cancel_speech`, `speak_ssml`, `get_process_id`, `set_on_ssml_mark_reached_callback`, plus `SpeechPriority` and `SymbolLevel` enums) — same surface as the existing example crate, but inlined.

CI behaviour: builds need outbound HTTPS to `download.nvaccess.org`. Airgapped or restricted CI can set `SAPIENCE_NVDA_CONTROLLER_DIR` to a pre-fetched mirror.

## COM entry points

### `DllMain`

- Store `HMODULE` in `AtomicIsize` (needed by `DllRegisterServer` to recover the DLL path via `GetModuleFileNameW`).
- Initialise `tracing` with a rolling file appender at `%TEMP%\SAPIence.log`. Read log level from `HKCU\Software\SAPIence\LogLevel`, then `HKLM`, default `WARN`.
- Install a panic hook that logs to `tracing::error!`.
- `DisableThreadLibraryCalls(hinst)`.

### `DllGetClassObject(rclsid, riid, ppv)`

- `rclsid != CLSID_SAPIENCE_VOICE` → `CLASS_E_CLASSNOTAVAILABLE`.
- `riid != IClassFactory::IID` → `E_UNEXPECTED`.
- Construct `ClassFactory`, write to `ppv`.

### `DllCanUnloadNow`

- Atomic ref counter incremented by `ClassFactory::CreateInstance`, decremented when each engine `Drop` runs. Return `S_OK` when zero, else `S_FALSE`.

### `DllRegisterServer` / `DllUnregisterServer`

No flags, no `DllInstall`. `regsvr32 sapience.dll` registers; `regsvr32 /u` unregisters.

Registration target: `HKEY_LOCAL_MACHINE` (matches SAPI convention; requires admin). All writes wrapped in a single `windows_registry::Transaction` for atomicity (same pattern as `rd_pipe-rs/src/registry.rs`).

Keys written:

```
HKLM\SOFTWARE\Classes\CLSID\{CLSID_SAPIENCE_VOICE}
    (Default)                = "SAPIence NVDA Voice"
    InprocServer32
        (Default)            = <full path to sapience.dll>
        ThreadingModel       = "Both"

HKLM\SOFTWARE\Microsoft\Speech\Voices\Tokens\SAPIence
    (Default)                = "SAPIence (NVDA)"
    CLSID                    = "{CLSID_SAPIENCE_VOICE}"
    LangDataPath             = ""
    VoiceDataPath            = ""
    Attributes
        Name                 = "SAPIence"
        Vendor               = "Leonard de Ruijter"
        Age                  = "Adult"
        Gender               = "Neutral"
        Language             = "409"
        Version              = "1.0"
```

`DllUnregisterServer` deletes both subtrees.

The CLSID is a compile-time constant: `pub const CLSID_SAPIENCE_VOICE: GUID = GUID::from_u128(0x...);` — generated once and committed.

## Class factory + loop prevention

```rust
#[implement(IClassFactory)]
pub struct ClassFactory;

impl IClassFactory_Impl for ClassFactory_Impl {
    fn CreateInstance(&self, outer: Ref<IUnknown>, iid: *const GUID, object: *mut *mut c_void)
        -> Result<()>
    {
        if outer.is_some() { return Err(Error::from(CLASS_E_NOAGGREGATION)); }

        // Loop prevention: if our process IS NVDA, refuse.
        // In-proc COM means the caller is in our process.
        if let Ok(nvda_pid) = nvda::get_process_id() {
            if nvda_pid == unsafe { GetCurrentProcessId() } {
                return Err(Error::from(CLASS_E_CLASSNOTAVAILABLE));
            }
        }
        // If NVDA isn't running, get_process_id errors — allow construction.

        let engine: TtsEngine = TtsEngine::new();
        match unsafe { *iid } {
            IUnknown::IID         => write_iface::<IUnknown>(engine, object),
            ISpTTSEngine::IID     => write_iface::<ISpTTSEngine>(engine, object),
            ISpObjectWithToken::IID => write_iface::<ISpObjectWithToken>(engine, object),
            _ => Err(Error::from(E_NOINTERFACE)),
        }
    }
}
```

## TtsEngine

Implements both `ISpTTSEngine` and `ISpObjectWithToken`. Holds an `Option<ISpObjectToken>` for the token SAPI passes in.

### `ISpObjectWithToken`

`SetObjectToken` stores; `GetObjectToken` returns the stored token.

### `GetOutputFormat`

Always returns `SPDFID_WaveFormatEx` with a `WAVEFORMATEX` describing 22050 Hz, 16-bit mono PCM. The buffer is allocated via `CoTaskMemAlloc` (SAPI takes ownership).

### `Speak(flags, format_id, wfx, frag_list, site)`

Top-level flow:

1. If `nvda::test_if_running()` returns `Err`, log a warning and return `S_OK` immediately. The host sees an empty utterance, no audio.
2. Read `interest = site.GetEventInterest()`. Use it to gate `AddEvents` calls.
3. Walk the singly-linked `SPVTEXTFRAG` list. Maintain a running `audio_offset: u64` (cumulative bytes written) for event positioning.
4. Per fragment, dispatch on `State.eAction`:
   - `SPVA_Speak` → speak via NVDA + pace.
   - `SPVA_Silence` → no NVDA call, pace `State.SilenceMSecs` of zero PCM.
   - `SPVA_Bookmark` → queue bookmark name; emitted as `<mark>` inside the *next* `SPVA_Speak` fragment (or as a synthesised one-mark utterance if no following speak fragment exists).
   - `SPVA_Pronounce` → treat as `SPVA_Speak` with plain text; no phoneme support.
   - `SPVA_SpellOut` → wrap text in `<say-as interpret-as="characters">`.
   - `SPVA_Section`, `SPVA_ParseUnknownTag` → ignore (continue).
5. Return `S_OK` after all fragments.

### Prosody mapping (`ssml.rs`)

Per the "limitations" requirement: ignore `site.GetRate()` and `site.GetVolume()`; only honour per-fragment `State.RateAdj`, `State.Volume`, `State.PitchAdj`.

| SAPI field           | Range  | SSML attribute       | Mapping                                      |
|----------------------|--------|----------------------|----------------------------------------------|
| `State.RateAdj`      | -10..10 | `<prosody rate="N%">` | `N = round(100 * 1.1^RateAdj)` (≈50%..260%) |
| `State.Volume`       | 0..100 | `<prosody volume="N">`| Pass-through                                 |
| `State.PitchAdj`     | -10..10 | `<prosody pitch="±Nst">` | 1 semitone per unit                       |

Emit a `<prosody>` wrapper only when at least one adjustment is non-default. Otherwise emit bare text inside `<speak>`.

SSML skeleton per fragment:

```xml
<speak version="1.0" xml:lang="en-US">
  <mark name="bm_K_<userbookmark>"/>     <!-- queued bookmarks -->
  <prosody rate="110%" pitch="+2st" volume="80">
    word1<mark name="w_K_0"/> word2<mark name="w_K_1"/> ...
  </prosody>
  <mark name="end_K"/>
</speak>
```

- `K` is a monotonic `AtomicU64` per-utterance counter (`UTT_COUNTER`).
- Words split on Unicode whitespace; a mark is inserted after each word.
- All text is XML-escaped (`&`, `<`, `>`, `"`).
- `xml:lang` derived from `State.LangID` (fallback `en-US`).

### Pacing (`pacing.rs`)

Silent PCM is written at the advertised format (22050 Hz, 16-bit mono = 44100 B/s). Chunk = 50 ms = 2205 samples = 4410 B.

```text
loop:
    if mark_state.end_reached or elapsed > safety_cap:
        break
    write_zero_chunk(site)        # SAPI flow-controls naturally
    audio_offset += 4410
    match site.GetActions():
        SPVES_ABORT  → nvda::cancel_speech(); return Aborted
        SPVES_SKIP   → handle_skip(site, frags); return SkipAdvanced
        SPVES_RATE/VOLUME → ignored (limitation: prosody-only)
    drain_new_marks(mark_state):  # see Marks
        for word_mark in new word marks since last drain:
            AddEvents(SPEI_WORD_BOUNDARY at audio_offset)
        for bookmark in new bookmark marks:
            AddEvents(SPEI_TTS_BOOKMARK at audio_offset, lParam = name)
```

Safety cap: `200 ms × character_count`. If exceeded, log a warning and return without further waiting (assume NVDA dropped the utterance). Without the cap, a crashed NVDA could hang Speak indefinitely.

### Marks (`marks.rs`)

NVDA's mark callback is process-global with a single function-pointer slot. Registered lazily on first `Speak` (via `std::sync::Once`), never unregistered for the DLL's lifetime.

```rust
static UTT_COUNTER: AtomicU64 = AtomicU64::new(0);
static MARK_REGISTRY: LazyLock<RwLock<HashMap<u64, Arc<MarkChannel>>>>
    = LazyLock::new(Default::default);

struct MarkChannel {
    inner: Mutex<MarkChannelInner>,
    cv: Condvar,
}
struct MarkChannelInner {
    new_words: Vec<u32>,        // word index
    new_bookmarks: Vec<String>, // user-provided name
    end_reached: bool,
}

extern "system" fn on_mark(name: *const wchar_t) {
    // Parse "w_K_N", "end_K", "bm_K_<name>".
    // Look up MarkChannel by K, signal cv.
}
```

`Speak` registers its channel under a fresh `K` before calling `speak_ssml`, drains marks each pacing iteration, and removes the channel before returning.

### NVDA dispatch

`nvda::speak_ssml(ssml, SymbolLevel::Unchanged, SpeechPriority::Next, asynchronous=true, Some(on_mark))`.

- `asynchronous=true` keeps the SAPI worker thread free for `Write`-driven pacing and action polling.
- `priority=Next` queues without interrupting NVDA's current utterance.
- The mark callback is registered lazily on first `Speak` (process-global slot), not per call.

### SAPI actions

| Action          | Handling                                                       |
|-----------------|----------------------------------------------------------------|
| `SPVES_ABORT`   | `nvda::cancel_speech()`, return `S_OK` from `Speak`.           |
| `SPVES_SKIP`    | `GetSkipInfo`, cancel current NVDA utterance, advance fragment iterator over N sentence boundaries, `CompleteSkip(actual)`. |
| `SPVES_RATE`    | Ignored (prosody-only, baseline disabled per requirements).    |
| `SPVES_VOLUME`  | Ignored.                                                       |

## Error handling and edge cases

- **NVDA not running at `Speak` start**: return `S_OK` silently; do not fail the call (would break hosts).
- **NVDA dies mid-utterance**: `speak_ssml` errors, or end-mark never fires; safety cap closes the pacing loop.
- **`speak_ssml` error**: log, continue to next fragment.
- **Reentrancy**: multiple engine instances may `Speak` concurrently. The global mark registry is keyed by utterance counter, so callbacks route correctly.
- **Stale NVDA PID in refusal check**: between NVDA restarts, the controller client could briefly report a stale PID. Worst case: our process happens to share that PID — we wrongly refuse. The host falls back to another voice. Acceptable.
- **Unicode**: SSML built as Rust `String`, handed to NVDA via `HSTRING::from(&str)`. Inbound SAPI text (`SPVTEXTFRAG::pTextStart`, UTF-16) read via `windows` helpers (`PCWSTR`/`PWSTR` + `HSTRING`); no manual `from_utf16_lossy`.
- **Memory**: `SPVTEXTFRAG` list owned by SAPI; engine only borrows. The only allocations in the hot loop are SSML strings (once per fragment) and event structs (per word).

## Threading

`ThreadingModel = "Both"` in registry. `Speak` runs on SAPI's worker thread; no additional threads are spawned. NVDA's mark callback may fire on an arbitrary NVDA-controller thread — `MARK_REGISTRY` uses `parking_lot::RwLock` for the registry, `Mutex`+`Condvar` per channel.

## Build targets

| Target                       | Output       |
|------------------------------|--------------|
| `i686-pc-windows-msvc`       | `sapience.dll` (x86) |
| `x86_64-pc-windows-msvc`     | `sapience.dll` (x64) |
| `aarch64-pc-windows-msvc`    | `sapience.dll` (ARM64) |

`Cargo.toml` `[lib] crate-type = ["cdylib"]`. Release profile: `lto = true`, `codegen-units = 1`, `opt-level = "s"` (mirrors `rd_pipe-rs`).

The x86 build supports 32-bit SAPI hosts; the x64 build supports 64-bit hosts (Komplete Kontrol). Each registers under its own COM hive (x86 builds under `HKLM\SOFTWARE\Wow6432Node\Classes\CLSID`; x64 under `HKLM\SOFTWARE\Classes\CLSID`). `regsvr32` handles the hive selection based on which DLL is registered.

## Testing

### Unit tests

- `ssml.rs`: table-driven prosody mapping. XML escape edge cases. Mark-name parser round-trip.
- `pacing.rs`: chunk-byte math; abort behaviour with a fake site.
- `registry.rs`: expected key paths and value names (no real registry writes).
- `class_factory.rs`: construction, CLSID/IID matching logic (mirrors `rd_pipe-rs` factory tests).

### Integration tests (`tests/`)

Scaffolding ported directly from `rd_pipe-rs/tests/common/mod.rs`:

- `dll_path()` — resolves the built DLL via env var (`SAPIENCE_DLL_PATH`) → target-triple/profile path → fallback. Supports `aarch64`, `x86_64`, `x86`.
- `HkcuOverride` — `RegLoadAppKeyW` + `RegOverridePredefKey` RAII guard. Adapted to override `HKEY_LOCAL_MACHINE` (since SAPI tokens live there) for the registration test.
- `DllHandle` — `OnceLock`-cached, leaked `libloading::Library`, prevents `DllMain` re-init.
- `read_sapience_log_tail()` — tails `%TEMP%\SAPIence.log` for failure diagnostics.

Tests:

1. **`register_unregister.rs`** (`serial_test`): redirect HKLM, call `DllRegisterServer`, assert CLSID + voice token subtrees exist with expected values, call `DllUnregisterServer`, assert they are gone.
2. **`instantiate.rs`**: `DllGetClassObject(CLSID_SAPIENCE_VOICE)` → `CreateInstance(ISpTTSEngine::IID)` succeeds, then casts to `ISpObjectWithToken`.
3. **`nvda_pid_refusal.rs`**: inject a fake `nvda::get_process_id` (via a `#[cfg(test)]` hook or a test-only env-var override) returning `GetCurrentProcessId()`. `CreateInstance` must return `CLASS_E_CLASSNOTAVAILABLE`.
4. **`pacing_silence.rs`**: fake `ISpTTSEngineSite` (`#[implement(ISpTTSEngineSite)]`, same pattern as `rd_pipe-rs` `FakeVirtualChannel`). Capture `Write` buffers and `AddEvents` calls. Drive `Speak` directly with a small fragment list (mocking NVDA either by patching `nvda::speak_ssml` behind a trait, or by treating absence of NVDA as a valid silent path).

### Manual smoke test

- `examples/speak_via_sapi.rs`: `CoInitialize`, `ISpVoice::SetVoice(SAPIence token)`, `Speak("Hello world")`, observe NVDA speaks.
- Run from inside NVDA (e.g., via NVDA's Python console invoking COM): instantiation must fail.

### CI

- Matrix: x86, x64, aarch64.
- All unit and integration tests except those requiring live NVDA.
- Live-NVDA smoke gated by `SAPIENCE_TEST_WITH_NVDA=1`.

## Open questions

None at design time. Implementation may surface details around: which exact NVDA callback thread fires the mark (informs locking), and whether word-mark density causes NVDA to skip marks under load (would force a coarser granularity, e.g., sentence-level marks).
