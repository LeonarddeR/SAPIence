//! Smoke test: speak via SAPI using the SAPIence voice. Requires the engine
//! is registered (run `regsvr32 target\debug\sapience.dll` first).

use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::core::HSTRING;

fn main() -> windows::core::Result<()> {
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok()?;
    // Use SpVoice directly via COM automation.
    // We can't use ISpVoice::SetVoice easily here without enumerating tokens,
    // so just speak — if SAPIence is the default voice, NVDA will speak.
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
