# SAPIence add-on DLL bundling design

## Goals

- Ship the Rust SAPIence runtime inside the NVDA add-on produced from this monorepo.
- Package both supported desktop architectures in the add-on: `x64` and `x86`.
- Keep the add-on package self-contained by including both `sapience.dll` and `nvdaControllerClient.dll` for each architecture.
- Register both COM DLLs during add-on install and unregister both during add-on removal.
- Request elevation only once per install operation and once per uninstall operation.
- Keep the registration flow simple by implementing it entirely in `addon\installTasks.py`.

## Non-goals

- No ARM64 packaging or registration work.
- No checked-in prebuilt DLLs under `addon\dll`.
- No extra helper Python module for registration logic.
- No custom command-line flag protocol beyond what is minimally required to enter the single elevated execution path.

## Packaging layout

The built add-on will contain this runtime layout:

```text
addon\
└── dll\
    ├── x64\
    │   ├── sapience.dll
    │   └── nvdaControllerClient.dll
    └── x86\
        ├── sapience.dll
        └── nvdaControllerClient.dll
```

These files are build artifacts, not source-controlled binaries.

## Build and packaging flow

The NVDA add-on build is responsible for copying the already-built Rust outputs into `addon\dll\...` before the `.nvda-addon` archive is created.

Expected artifact sources:

- `target\x86_64-pc-windows-msvc\release\sapience.dll`
- `target\x86_64-pc-windows-msvc\release\nvdaControllerClient.dll`
- `target\i686-pc-windows-msvc\release\sapience.dll`
- `target\i686-pc-windows-msvc\release\nvdaControllerClient.dll`

The packaging step copies those files into:

- `addon\dll\x64\`
- `addon\dll\x86\`

The add-on build logic should fail clearly if any required source artifact is missing. It should not silently omit an architecture.

This keeps the source of truth in the Rust build while ensuring the shipped add-on contains the runtime files needed for registration and execution.

## Install and uninstall flow

`addon\installTasks.py` owns the runtime lifecycle actions.

During install:

1. Locate `sapience.dll` in `addon\dll\x64` and `addon\dll\x86`.
2. Enter a single elevated execution path using NVDA's elevated helper pattern.
3. Within that one elevated run, call the 64-bit `regsvr32.exe` for the x64 DLL and the 32-bit `regsvr32.exe` for the x86 DLL.
4. Abort the install if elevation is denied or either registration step fails.

During uninstall:

1. Enter a single elevated execution path using the same pattern.
2. Within that one elevated run, call the matching 64-bit and 32-bit `regsvr32.exe` instances with unregister semantics.
3. Abort the uninstall if elevation is denied or either unregister step fails.

The registration logic remains entirely in `installTasks.py`. It should not depend on a second helper module. The elevated path may use the minimum amount of internal branching needed to distinguish normal execution from elevated execution, but the design does not introduce user-facing flags or a reusable subcommand surface.

## Architecture-specific registration

The install task must explicitly use the correct system `regsvr32.exe` for each DLL architecture:

- x64 DLL -> 64-bit `regsvr32.exe`
- x86 DLL -> 32-bit `regsvr32.exe`

On 64-bit Windows, this means the implementation must deliberately reach the 64-bit and 32-bit system directories rather than relying on whatever `regsvr32.exe` happens to resolve first in the current process context.

## Component boundaries

The work is split into two focused responsibilities:

- **Build packaging responsibility:** gather the Rust-produced DLL artifacts and place them into `addon\dll\x64` and `addon\dll\x86` so they are included in the add-on archive.
- **Install task responsibility:** perform install-time registration and uninstall-time unregistration against the packaged DLL paths.

`installTasks.py` should treat the packaged DLL locations as inputs and should not know how the Rust project produced them. Likewise, the build packaging logic should not perform registration work.

## Failure handling

Failures are surfaced explicitly and stop the operation:

- Missing packaged DLLs
- Missing source build artifacts during packaging
- Elevation denied
- Failure of either architecture-specific `regsvr32` call

The design does not allow best-effort continuation after a partial registration result, because that would leave the machine in an ambiguous state.

## Validation

The completed work is successful when all of the following are true:

- The built `.nvda-addon` contains:
  - `dll\x64\sapience.dll`
  - `dll\x64\nvdaControllerClient.dll`
  - `dll\x86\sapience.dll`
  - `dll\x86\nvdaControllerClient.dll`
- Installing the add-on performs one elevation flow and registers both DLL variants.
- Removing the add-on performs one elevation flow and unregisters both DLL variants.
- A failure in any required step stops the operation clearly instead of continuing in a partially completed state.
