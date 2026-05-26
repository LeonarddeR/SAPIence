# SAPIence

A SAPI 5 TTS engine that forwards speech to NVDA. Lets SAPI-only apps (e.g., Komplete Kontrol) speak through your screen reader.

## Build

```
cargo build --release
```

Targets: `x86_64-pc-windows-msvc`, `i686-pc-windows-msvc`, `aarch64-pc-windows-msvc`.

First build downloads `nvda_<version>_controllerClient.zip` from NV Access automatically.

Offline builds: set `SAPIENCE_NVDA_CONTROLLER_DIR` to a local pre-extracted copy of the controller client.

## Install

Place `sapience.dll` and `nvdaControllerClient.dll` in the same directory, then:

```
regsvr32 sapience.dll
```

The release build copies `nvdaControllerClient.dll` alongside `sapience.dll` in `target/<triple>/release/` automatically.

## Uninstall

```
regsvr32 /u sapience.dll
```

## Logging

Set `HKCU\Software\SAPIence\LogLevel` (string) to `TRACE`, `DEBUG`, `INFO`, `WARN`, or `ERROR`. Default: `WARN`. Log file: `%TEMP%\SAPIence.log`.

## License

LGPL-2.1-or-later.
