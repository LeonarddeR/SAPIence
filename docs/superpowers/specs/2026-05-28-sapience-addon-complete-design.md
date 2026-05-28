# SAPIence add-on complete design

## Goals

- Ship the Rust SAPIence runtime inside the NVDA add-on produced from this monorepo.
- Support only x64 Windows systems at add-on install time.
- Package both supported desktop architectures in the add-on: `x64` and `x86`.
- Keep the add-on package self-contained by including `sapience.dll` and `nvdaControllerClient.dll` for each architecture.
- Register both COM DLLs during add-on install and unregister both during add-on removal, with a single UAC elevation per operation.
- Inform the user why administrator privileges are required before requesting consent.
- Prevent NVDA from listing SAPIence as a selectable speech synthesizer for its own use.

## Non-goals

- No ARM64 packaging or registration work.
- No support for 32-bit Windows hosts.
- No checked-in prebuilt DLLs under `addon\dll`.
- No extra helper Python module for registration logic beyond `installTasks.py`.
- No local dev automation for placing DLLs — when running from source, copy them manually.

## Packaging layout

The built add-on will contain:

```text
addon\
├── installTasks.py
├── regsvrHelper.bat
├── synthDrivers\
│   └── sapi5.py
└── dll\
    ├── x64\
    │   ├── sapience.dll
    │   └── nvdaControllerClient.dll
    └── x86\
        ├── sapience.dll
        └── nvdaControllerClient.dll
```

DLL files are build artifacts, not source-controlled.

## Feature 1: CI workflow consolidation and DLL bundling

### Approach

Merge `build_addon.yml` into `ci.yml`. Delete `build_addon.yml`.

### New `build_addon` job in `ci.yml`

- `needs: [build]` — waits for all Rust matrix builds (x86, x64, arm64).
- Runs on `ubuntu-latest`.
- Downloads `sapience-x64` and `sapience-x86` artifacts (arm64 is not needed for the add-on).
- Extracts `sapience.dll` and `nvdaControllerClient.dll` from each artifact into:
  - `addon/dll/x64/`
  - `addon/dll/x86/`
- Runs Python checks: `export SKIP=no-commit-to-branch; uv run pre-commit run --all-files`
- Runs `uv run scons && uv run scons pot`
- Uploads `.nvda-addon` artifact (`packaged_addon`) and `.pot` artifact (`translation_template`).

### Updated `release` job in `ci.yml`

Merge the `upload_release` logic from the old `build_addon.yml` into the existing `release` job:

- `needs: [build, test, build_addon]`
- Downloads all artifacts.
- Uploads `*.nvda-addon`, `*.pot`, and the Rust zip to the GitHub Release.
- Appends SHA256 of `*.nvda-addon` to `changelog.md`.

### sconstruct DLL check

Add an explicit check at the top of the packaging step that verifies all four DLL files exist:

- `addon/dll/x64/sapience.dll`
- `addon/dll/x64/nvdaControllerClient.dll`
- `addon/dll/x86/sapience.dll`
- `addon/dll/x86/nvdaControllerClient.dll`

Fail with a clear error message if any is missing. Do not silently omit an architecture.

## Feature 2: Install and uninstall tasks

### `addon/regsvrHelper.bat`

Single bundled batch script. Accepts:

- `%1` — absolute path to x64 `sapience.dll`
- `%2` — absolute path to x86 `sapience.dll`
- `%3` — optional `/u` for unregistration (omit for registration)

```batch
@echo off
"%SystemRoot%\System32\regsvr32.exe" %3 /s "%1"
if %errorlevel% neq 0 exit /b %errorlevel%
"%SystemRoot%\SysWOW64\regsvr32.exe" %3 /s "%2"
if %errorlevel% neq 0 exit /b %errorlevel%
```

Runs both architectures in one elevated invocation. Aborts on first failure.

### `addon/installTasks.py`

`onInstall` and `onUninstall` are the only public symbols. Both follow the same structure:

1. Resolve paths via `addonHandler.getCodeAddon().path` (matching the rdPipe pattern).
2. Verify both DLL files exist with `os.path.isfile()` — raise `FileNotFoundError` if missing.
3. Show a `MessageDialog` (Yes/No, `DialogType.WARNING`) with a message along the lines of: "SAPIence must register itself as a SAPI synthesizer in the system-wide registry. This requires administrator privileges. Do you want to continue?" Yes = proceed, No = raise to abort the operation.
5. Call `systemUtils.execElevated(batch_path, [dll_x64, dll_x86[, "/u"]], wait=True)`.
   - UAC denial raises `OSError` — propagate naturally.
6. Check return code — non-zero raises `RuntimeError`.

`regsvrHelper.bat` path is also resolved via `addonHandler.getCodeAddon().path`.

## Feature 3: SAPI5 driver override

### Purpose

Prevent NVDA from listing SAPIence as a selectable speech synthesizer for its own use. SAPIence is registered system-wide as a SAPI voice; without this override, NVDA's SAPI5 driver would offer it as a voice option, creating a feedback loop.

### `addon/synthDrivers/sapi5.py`

```python
import nvdaBuiltin.synthDrivers.sapi5 as _builtin

_SAPIENCE_TOKEN_SUFFIX = "SAPIence"

class SynthDriver(_builtin.SynthDriver):
    def _getVoiceTokens(self):
        tokens = super()._getVoiceTokens()
        return [
            tokens[i]
            for i in range(len(tokens))
            if not tokens[i].Id.endswith(_SAPIENCE_TOKEN_SUFFIX)
        ]
```

Returns a Python list. All callers in `sapi5.py` use only `len()` and index access, so a list is a valid drop-in for the COM token collection.

`_SAPIENCE_TOKEN_SUFFIX` matches `VOICE_TOKEN_NAME` in `src/clsid.rs`.

### `buildVars.py`

Add `synthDrivers/sapi5.py` to `pythonSources`. Verify whether SCons auto-discovers files in `synthDrivers/` or requires explicit listing — if auto-discovered, this step is unnecessary.

## Failure handling

All failures are surfaced explicitly and stop the operation:

- Missing DLL files at package time (sconstruct check)
- Missing DLL files at install/uninstall time (`os.path.isfile` check)
- User declines the consent dialog
- UAC elevation denied (propagated `OSError`)
- Non-zero exit from either `regsvr32` call

## Validation

The completed work is successful when:

- The built `.nvda-addon` contains all four DLL files plus `regsvrHelper.bat` and `synthDrivers/sapi5.py`.
- Installing the add-on shows the admin-required dialog, then performs one UAC elevation, then registers both DLL variants.
- Removing the add-on follows the same single-elevation flow and unregisters both variants.
- NVDA's SAPI5 synthesizer does not list SAPIence as a voice option.
- A failure in any required step stops the operation clearly.
