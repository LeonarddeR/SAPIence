//! Smoke test: speak via SAPI using the SAPIence voice. Requires the engine
//! is registered (run `regsvr32 target\debug\sapience.dll` first) and NVDA
//! is running.

use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::core::HSTRING;

fn main() -> windows::core::Result<()> {
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok()?;
    let voice: windows::Win32::Media::Speech::ISpVoice = unsafe {
        windows::Win32::System::Com::CoCreateInstance(
            &windows::Win32::Media::Speech::SpVoice,
            None,
            windows::Win32::System::Com::CLSCTX_ALL,
        )
    }?;
    let text = HSTRING::from("Hello from SAPIence. NVDA should be speaking this.");
    unsafe { voice.Speak(&text, 0, None) }?;
    Ok(())
}
