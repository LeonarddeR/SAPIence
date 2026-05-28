# SAPIence add-on complete design — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the Rust SAPIence runtime as a self-contained NVDA add-on that bundles both x64 and x86 DLLs, registers them with one UAC elevation, and prevents NVDA's own SAPI5 driver from listing SAPIence as a voice.

**Architecture:**
CI builds the Rust DLLs per-arch, then a consolidated `build_addon` job downloads the x64/x86 artifacts and lays them out under `addon/dll/<arch>/` before invoking SCons. The add-on contains `installTasks.py` which resolves bundled DLL paths, prompts the user, and runs a single elevated `regsvrHelper.bat` that calls both `System32` and `SysWOW64` `regsvr32.exe`. A `synthDrivers/sapi5.py` subclass filters the SAPIence voice token out of NVDA's own voice list.

**Tech Stack:** Rust (cdylib), Python (NVDA add-on host), Windows batch, SCons, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-05-28-sapience-addon-complete-design.md`

**Note on testing:** Python-side code (`installTasks.py`, `synthDrivers/sapi5.py`) runs inside the NVDA host and depends on `addonHandler`, `systemUtils`, `gui.message`, `nvdaBuiltin` — none available outside NVDA. There is no pytest harness in this repo. Verification for these modules is manual: install the packaged `.nvda-addon` in NVDA, observe the dialog, accept UAC, confirm both regsvr32 invocations succeed, then confirm NVDA's SAPI5 list omits SAPIence. CI verifies workflow + sconstruct + ruff via existing `pre-commit run --all-files`.

---

## File Structure

**Create:**
- `addon/regsvrHelper.bat` — bundled registration helper, both archs.
- `addon/synthDrivers/sapi5.py` — NVDA SAPI5 driver override filtering SAPIence token.

**Modify:**
- `addon/installTasks.py` — implement `onInstall` and `onUninstall`.
- `buildVars.py` — add `synthDrivers/sapi5.py` (and `installTasks.py`) to `pythonSources` if SCons does not auto-discover them.
- `sconstruct` — add DLL-existence check before packaging.
- `.github/workflows/ci.yml` — add `build_addon` job; extend `release` job to include addon + .pot uploads and SHA256 changelog.
- `changelog.md` — leave as-is; CI appends SHA256 at release time.

**Delete:**
- `.github/workflows/build_addon.yml` — merged into `ci.yml`.

---

## Task 1: sconstruct DLL existence check

**Files:**
- Modify: `sconstruct` (insert after the `addonDir` declaration, before `env.NVDAAddon(...)`)

- [ ] **Step 1: Add the check block**

Insert this block in `sconstruct` immediately after the `addonDir: Final = Path("addon/")` line (around line 48), so it runs before any SCons targets are constructed:

```python
# Verify bundled DLLs exist before packaging.
# DLLs are not source-controlled; CI lays them out from per-arch build artifacts.
_REQUIRED_DLLS: Final = [
    addonDir / "dll" / "x64" / "sapience.dll",
    addonDir / "dll" / "x64" / "nvdaControllerClient.dll",
    addonDir / "dll" / "x86" / "sapience.dll",
    addonDir / "dll" / "x86" / "nvdaControllerClient.dll",
]
_missingDlls = [str(p) for p in _REQUIRED_DLLS if not p.is_file()]
if _missingDlls:
    raise ValueError(
        "Cannot package add-on: required DLL files are missing:\n  "
        + "\n  ".join(_missingDlls)
        + "\nCI populates addon/dll/<arch>/ from sapience-x64 / sapience-x86 artifacts; "
          "for local builds, copy them in by hand."
    )
```

- [ ] **Step 2: Verify the check fires when DLLs are absent**

Run: `uv run scons`
Expected: `ValueError` with the listed missing paths.

- [ ] **Step 3: Stage placeholder DLLs and verify SCons proceeds past the check**

Create empty placeholders so the check passes (these will be replaced by real CI artifacts):

```powershell
New-Item -ItemType Directory -Force addon/dll/x64, addon/dll/x86 | Out-Null
foreach ($a in 'x64','x86') {
    foreach ($n in 'sapience.dll','nvdaControllerClient.dll') {
        New-Item -ItemType File "addon/dll/$a/$n" | Out-Null
    }
}
```

Run: `uv run scons`
Expected: build proceeds past the DLL check (later targets may still fail — only the check itself must pass). Then clean up so placeholders are not committed:

```powershell
Remove-Item -Recurse -Force addon/dll
```

- [ ] **Step 4: Commit**

```bash
git add sconstruct
git commit -m "build: fail SCons packaging when bundled DLLs are missing"
```

---

## Task 2: Update `.gitignore` for `addon/dll/`

**Files:**
- Modify: `.gitignore`

- [ ] **Step 1: Confirm whether `addon/dll/` is already ignored**

Run: `git check-ignore -v addon/dll/x64/sapience.dll`
If output indicates a matching rule, skip to commit-less step 3.

- [ ] **Step 2: Add ignore entry**

Append to `.gitignore`:

```
# Bundled DLLs are CI artifacts; copy in by hand for local builds.
/addon/dll/
```

- [ ] **Step 3: Commit (only if `.gitignore` changed)**

```bash
git add .gitignore
git commit -m "build: ignore addon/dll/ — populated by CI, not source-controlled"
```

---

## Task 3: `regsvrHelper.bat`

**Files:**
- Create: `addon/regsvrHelper.bat`

- [ ] **Step 1: Create the helper script**

Write `addon/regsvrHelper.bat` exactly as:

```batch
@echo off
"%SystemRoot%\System32\regsvr32.exe" %3 /s "%1"
if %errorlevel% neq 0 exit /b %errorlevel%
"%SystemRoot%\SysWOW64\regsvr32.exe" %3 /s "%2"
if %errorlevel% neq 0 exit /b %errorlevel%
```

Notes for the engineer:
- `System32` on 64-bit Windows hosts x64 binaries; `SysWOW64` hosts x86 binaries. Both must run.
- `%3` is the optional `/u` flag for unregister. Empty when registering.
- `/s` runs silently — no message box.
- Save with CRLF line endings (Windows batch).

- [ ] **Step 2: Smoke-test the script with placeholder args (no actual registration)**

Run from a non-elevated cmd:

```cmd
addon\regsvrHelper.bat C:\Windows\System32\nonexistent.dll C:\Windows\SysWOW64\nonexistent.dll
```

Expected: exit code non-zero; no crash, no message boxes (the `/s` flag suppresses them).

- [ ] **Step 3: Commit**

```bash
git add addon/regsvrHelper.bat
git commit -m "feat(addon): add regsvrHelper.bat for single-elevation x64+x86 registration"
```

---

## Task 4: `installTasks.py`

**Files:**
- Modify: `addon/installTasks.py` (currently a single empty line)

- [ ] **Step 1: Write `installTasks.py`**

Replace the file contents with:

```python
# Registers (or unregisters) the bundled x64 and x86 sapience.dll COM servers
# with a single UAC elevation per operation.

import os

import addonHandler
import systemUtils
from gui.message import DialogType, MessageDialog, ReturnCode


_addon = addonHandler.getCodeAddon()
_ADDON_PATH = _addon.path
_CMD = os.path.join(os.environ["SystemRoot"], "System32", "cmd.exe")
_HELPER_BAT = os.path.join(_ADDON_PATH, "regsvrHelper.bat")
_DLL_X64 = os.path.join(_ADDON_PATH, "dll", "x64", "sapience.dll")
_DLL_X86 = os.path.join(_ADDON_PATH, "dll", "x86", "sapience.dll")


def _verifyBundledFiles() -> None:
    for path in (_HELPER_BAT, _DLL_X64, _DLL_X86):
        if not os.path.isfile(path):
            raise FileNotFoundError(path)


def _askConsent() -> None:
    # Translators: Title of the dialog asking for consent to elevate during add-on install/uninstall.
    title = _("{addonName}: administrator privileges required").format(addonName=_addon.name)
    # Translators: Body of the consent dialog explaining why elevation is needed.
    body = _(
        "{addonName} must register itself as a SAPI synthesizer in the system-wide registry. "
        "This requires administrator privileges. Do you want to continue?"
    ).format(addonName=_addon.name)
    dlg = MessageDialog(
        parent=None,
        message=body,
        title=title,
        dialogType=DialogType.WARNING,
        buttons=None,
    ).addYesButton().addNoButton(defaultFocus=True)
    if dlg.ShowModal() != ReturnCode.YES:
        raise RuntimeError("User declined elevation.")


def _runHelper(unregister: bool) -> None:
    # execElevated uses ShellExecuteEx/runas — invoke via cmd.exe so the .bat
    # file association is resolved correctly regardless of NVDA's shell context.
    args = ["/c", _HELPER_BAT, _DLL_X64, _DLL_X86]
    if unregister:
        args.append("/u")
    rc = systemUtils.execElevated(_CMD, args, wait=True)
    if rc != 0:
        raise RuntimeError(f"regsvrHelper.bat exited with code {rc}.")


def onInstall() -> None:
    _verifyBundledFiles()
    _askConsent()
    _runHelper(unregister=False)


def onUninstall() -> None:
    _verifyBundledFiles()
    _askConsent()
    _runHelper(unregister=True)
```

Notes for the engineer:
- `addonHandler.getCodeAddon()` returns an `Addon` object; `.path` is the on-disk directory, `.name` is the internal add-on identifier from `manifest.ini` — matches the `addon_name` in `buildVars.py`.
- `cmd.exe /c <bat>` is required because `systemUtils.execElevated` calls `ShellExecuteEx` with the `runas` verb; batch files are not directly executable by `CreateProcess` and shell-association handling under `runas` is unreliable without an explicit host.
- `systemUtils.execElevated` raises `OSError` (`WinError 1223`) when the user denies UAC; propagate it.
- `MessageDialog` builder API: `addYesButton()` / `addNoButton(defaultFocus=True)`. Defaulting focus to "No" follows the destructive-action convention.
- `_(...)` is a builtin translation lookup in NVDA add-on context — no import needed.

- [ ] **Step 2: Lint**

Run: `uv run pre-commit run --all-files`
Expected: pass (or only pre-existing failures unrelated to this file).

- [ ] **Step 3: Commit**

```bash
git add addon/installTasks.py
git commit -m "feat(addon): register/unregister SAPI DLLs on install/uninstall"
```

---

## Task 5: SAPI5 driver override

**Files:**
- Create: `addon/synthDrivers/sapi5.py`

- [ ] **Step 1: Create the driver subclass**

Write `addon/synthDrivers/sapi5.py`:

```python
# Subclass NVDA's built-in SAPI5 driver and hide the SAPIence voice token,
# so NVDA does not list SAPIence as a selectable synthesizer for its own output.

from nvdaBuiltin.synthDrivers.sapi5 import *  # noqa: F401,F403


_SAPIENCE_TOKEN_SUFFIX = "SAPIence"


class SynthDriver(SynthDriver):  # type: ignore[no-redef]  # noqa: F811
    def _getVoiceTokens(self):
        tokens = super()._getVoiceTokens()
        return [
            tokens[i]
            for i in range(len(tokens))
            if not tokens[i].Id.endswith(_SAPIENCE_TOKEN_SUFFIX)
        ]
```

Notes:
- Wildcard import re-exports all public names from the builtin driver (constants, `SynthDriver`, etc.) so NVDA's driver loader sees the full module API. Without it, only `SynthDriver` would be present and any driver-level constants the loader expects would be missing.
- `class SynthDriver(SynthDriver)` shadows the imported name; this is the standard NVDA override pattern. `F811` (redefinition) and `F401`/`F403` (wildcard) are suppressed with `noqa`.
- `_SAPIENCE_TOKEN_SUFFIX` must match `VOICE_TOKEN_NAME` in `src/clsid.rs` (currently `"SAPIence"`).
- Returns a plain list; the built-in driver only uses `len()` and index access on the returned object.
- `tokens[i].Id` is the COM token's `Id` property (full registry path ending in the token name).

- [ ] **Step 3: Lint**

Run: `uv run pre-commit run --all-files`
Expected: pass.

- [ ] **Step 4: Commit**

```bash
git add addon/synthDrivers/sapi5.py
git commit -m "feat(addon): hide SAPIence voice from NVDA's own SAPI5 driver"
```

---

## Task 6: `buildVars.py` — sync metadata with `Cargo.toml` and add python sources

**Files:**
- Modify: `buildVars.py`

- [ ] **Step 1: Determine whether SCons auto-discovers Python files under `addon/`**

Inspect `site_scons/site_tools/NVDATool/` — does the `NVDAAddon` builder copy the entire `addon/` tree, or only files listed in `pythonSources`? `pythonSources` drives `i18nSources` (translation extraction) regardless.

Run: `Get-ChildItem -Recurse site_scons/site_tools/NVDATool | Select-String -Pattern "pythonSources|\.py" -List`
Read each match and decide.

- [ ] **Step 2: Update `buildVars.py` to match `Cargo.toml` and add python sources**

`Cargo.toml` fields (exact values as of this writing):
- `name = "sapience"`
- `version = "0.1.0"`
- `description = "A SAPI 5 TTS engine that forwards speech to NVDA"`
- `authors = ["Leonard de Ruijter <l.de.ruijter@sevenp.nl>"]`
- `license = "LGPL-2.1-or-later"`

Apply these to `buildVars.py`:

```python
addon_info = AddonInfo(
    addon_name="sapience",
    addon_summary=_("SAPIence"),
    addon_description=_("A SAPI 5 TTS engine that forwards speech to NVDA"),
    addon_version="0.1.0",
    addon_changelog=_(""),
    addon_author="Leonard de Ruijter <alderuijter@gmail.com>",
    addon_url=None,
    addon_sourceURL="https://github.com/leonardder/sAPIence",
    addon_docFileName="readme.html",
    addon_minimumNVDAVersion=None,
    addon_lastTestedNVDAVersion=None,
    addon_updateChannel=None,
    addon_license="LGPL-2.1-or-later",
    addon_licenseURL=None,
)

pythonSources: list[str] = [
    "addon/installTasks.py",
    "addon/synthDrivers/*.py",
]
```

- [ ] **Step 3: Run pot generation to verify the new strings are extracted**

Run: `uv run scons pot`
Expected: `sapience.pot` (now that `addon_name` is `"sapience"`) exists and contains consent strings.

Check with: `Select-String -Path "*.pot" -Pattern "administrator privileges required"`
Expected: one match.

- [ ] **Step 4: Commit**

```bash
git add buildVars.py
git commit -m "build: sync buildVars metadata with Cargo.toml; add python sources for i18n"
```

---

## Task 7: CI — add `build_addon` job, extend `release`, delete old workflow

**Files:**
- Modify: `.github/workflows/ci.yml`
- Delete: `.github/workflows/build_addon.yml`

- [ ] **Step 1: Add `build_addon` job to `ci.yml`**

Insert this job after the `test` job and before the `release` job:

```yaml
  build_addon:
    name: Build add-on
    needs: build
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6

      - name: Install uv
        uses: astral-sh/setup-uv@v7
        with:
          enable-cache: true

      - name: Install system dependencies
        run: |
          sudo apt-get update -y
          sudo apt-get install -y gettext

      - name: Install Python dependencies
        run: uv sync

      - name: Download x64 DLLs
        uses: actions/download-artifact@v8
        with:
          name: sapience-x64
          path: addon/dll/x64

      - name: Download x86 DLLs
        uses: actions/download-artifact@v8
        with:
          name: sapience-x86
          path: addon/dll/x86

      - name: Strip non-DLL files from artifact payload
        run: |
          set -euo pipefail
          for arch in x64 x86; do
            find "addon/dll/${arch}" -mindepth 1 -maxdepth 1 \
              ! -name 'sapience.dll' ! -name 'nvdaControllerClient.dll' \
              -delete
          done
          ls -lR addon/dll

      - name: Set addon version from tag
        if: startsWith(github.ref, 'refs/tags/v')
        run: |
          VERSION="${GITHUB_REF_NAME#v}"
          sed -i "s/addon_version=\"[^\"]*\"/addon_version=\"${VERSION}\"/" buildVars.py

      - name: Code checks
        run: SKIP=no-commit-to-branch uv run pre-commit run --all-files

      - name: Build add-on
        run: uv run scons && uv run scons pot

      - uses: actions/upload-artifact@v7
        with:
          name: packaged_addon
          path: ./*.nvda-addon
          compression-level: 9

      - uses: actions/upload-artifact@v7
        with:
          name: translation_template
          path: ./*.pot
          compression-level: 9
```

Notes for the engineer:
- The `build` job uploads `*.dll`, `*.dll.lib`, and `*.pdb`. The strip step keeps only the two DLLs we ship — `.lib` and `.pdb` must not be packaged.
- `SKIP=no-commit-to-branch` matches the old workflow's environment variable so pre-commit's branch-protection hook is bypassed in CI.
- The version step strips the leading `v` from the tag (e.g. `v0.2.0` → `0.2.0`) and patches `addon_version` in `buildVars.py` in-place before SCons runs. This mirrors the `cargo set-version` step in the `build` job and keeps both manifests in sync on release.

- [ ] **Step 2: Extend `release` job**

Replace the existing `release:` block (lines 127-159) with:

```yaml
  release:
    name: Release
    needs: [build, test, build_addon]
    if: startsWith(github.ref, 'refs/tags/v')
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - uses: actions/checkout@v6

      - name: Download all artifacts
        uses: actions/download-artifact@v8
        with:
          path: artifacts

      - name: Assemble Rust release zip
        run: |
          set -euo pipefail
          mkdir -p release
          for suffix in x86 x64 arm64; do
            src="artifacts/sapience-${suffix}"
            [ -d "$src" ] || { echo "Missing artifact: $src" >&2; exit 1; }
            mv "$src" "release/${suffix}"
          done
          (cd release && zip -r -9 "../sapience-${GITHUB_REF_NAME}.zip" .)

      - name: Stage addon + pot artifacts
        run: |
          set -euo pipefail
          mv artifacts/packaged_addon/*.nvda-addon .
          mv artifacts/translation_template/*.pot .

      - name: Append SHA256 of .nvda-addon to changelog
        run: |
          set -euo pipefail
          printf '\nSHA256:\n' >> changelog.md
          sha256sum *.nvda-addon >> changelog.md

      - name: Publish GitHub Release
        uses: softprops/action-gh-release@v3
        with:
          files: |
            sapience-*.zip
            *.nvda-addon
            *.pot
          body_path: changelog.md
          generate_release_notes: true
          fail_on_unmatched_files: true
          name: ${{ github.ref_name }}
          prerelease: ${{ contains(github.ref, '-') }}
```

- [ ] **Step 3: Delete the old standalone workflow**

```bash
git rm .github/workflows/build_addon.yml
```

- [ ] **Step 4: Validate the YAML locally**

Run: `uv run python -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"`
Expected: no exception.

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: consolidate addon build into ci.yml; bundle x64+x86 DLLs in release"
```

---

## Task 8: End-to-end validation

This task is manual — it cannot be automated from the repo. Do not skip it.

- [ ] **Step 1: Trigger a CI run on the branch**

Push the branch, watch the `build_addon` job pass, download the `packaged_addon` artifact.

- [ ] **Step 2: Inspect the `.nvda-addon` contents**

```powershell
Expand-Archive -Path .\SAPIence-*.nvda-addon -DestinationPath .\addon-inspect -Force
Get-ChildItem -Recurse .\addon-inspect | Select-Object FullName
```

Expected entries present:
- `installTasks.py`
- `regsvrHelper.bat`
- `synthDrivers\sapi5.py`
- `dll\x64\sapience.dll`
- `dll\x64\nvdaControllerClient.dll`
- `dll\x86\sapience.dll`
- `dll\x86\nvdaControllerClient.dll`
- `manifest.ini`

- [ ] **Step 3: Install in NVDA**

In NVDA: Tools → Add-on Store → Install from external source → pick the `.nvda-addon`.
Expected order:
1. Consent dialog appears with the admin-required message.
2. Clicking Yes triggers a single UAC prompt.
3. After consent, registration completes without further prompts.

- [ ] **Step 4: Verify SAPIence is visible to SAPI clients but hidden from NVDA's SAPI5 driver**

- Open NVDA's synthesizer dialog → SAPI 5 → confirm SAPIence is **not** listed.
- Open Windows Narrator voice list or any SAPI 5 client → SAPIence **is** listed.

- [ ] **Step 5: Speak via a SAPI client and confirm NVDA voices the SSML**

Use the `examples/` test client (or any SAPI app) targeting the SAPIence voice. NVDA should announce the text.

- [ ] **Step 6: Remove the add-on**

Tools → Add-on Store → SAPIence → Remove. Restart NVDA when prompted.
Expected: same consent dialog, one UAC prompt, clean unregistration.

- [ ] **Step 7: Confirm registry cleanup**

```powershell
Get-Item 'HKLM:\SOFTWARE\Microsoft\Speech\Voices\Tokens\SAPIence' -ErrorAction SilentlyContinue
Get-Item 'HKLM:\SOFTWARE\Classes\CLSID\{5A91E9CE-2BC7-4F8E-9DA1-4D7C9F2E7E11}' -ErrorAction SilentlyContinue
```

Expected: both return `$null`.

- [ ] **Step 8: Failure path — decline consent**

Re-install. At the consent dialog click No.
Expected: NVDA reports the add-on install failed; no UAC prompt was shown; no registry entries created.

- [ ] **Step 9: Failure path — deny UAC**

Re-install. At the consent dialog click Yes, then decline UAC.
Expected: NVDA reports failure; no registry entries created; the error message references the elevation failure.

---

## Self-review checklist (already applied)

- Spec coverage: feature 1 → tasks 1, 7; feature 2 → tasks 3, 4; feature 3 → tasks 5, 6; validation → task 8. All spec bullets accounted for.
- `_SAPIENCE_TOKEN_SUFFIX` value (`"SAPIence"`) matches `VOICE_TOKEN_NAME` in `src/clsid.rs:14`.
- `pythonSources` updated explicitly so consent-dialog strings reach the `.pot` file regardless of SCons auto-discovery.
- DLL existence is checked at three boundaries: CI artifact download (implicit — missing artifact fails the job), sconstruct (task 1), install-time `os.path.isfile` (task 4).
- Release job depends on `build_addon` so a packaging failure blocks the release.
