//! Stub for the TTS engine — full implementation in a later task.

use windows::Win32::Media::Speech::{ISpObjectWithToken, ISpTTSEngine};
use windows::core::implement;

#[implement(ISpTTSEngine, ISpObjectWithToken)]
pub struct TtsEngine;

impl TtsEngine {
    pub fn new() -> Self {
        TtsEngine
    }
}

impl windows::Win32::Media::Speech::ISpTTSEngine_Impl for TtsEngine_Impl {
    fn Speak(
        &self,
        _dwspeakflags: u32,
        _rguidformatid: *const windows::core::GUID,
        _pwaveformatex: *const windows::Win32::Media::Audio::WAVEFORMATEX,
        _ptextfraglist: *const windows::Win32::Media::Speech::SPVTEXTFRAG,
        _poutputsite: windows::core::Ref<windows::Win32::Media::Speech::ISpTTSEngineSite>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn GetOutputFormat(
        &self,
        _ptargetfmtid: *const windows::core::GUID,
        _ptargetwaveformatex: *const windows::Win32::Media::Audio::WAVEFORMATEX,
        _pdesiredfmtid: *mut windows::core::GUID,
        _ppcomemdesiredwaveformatex: *mut *mut windows::Win32::Media::Audio::WAVEFORMATEX,
    ) -> windows::core::Result<()> {
        Err(windows::core::Error::from(windows::Win32::Foundation::E_NOTIMPL))
    }
}

impl windows::Win32::Media::Speech::ISpObjectWithToken_Impl for TtsEngine_Impl {
    fn SetObjectToken(
        &self,
        _ptoken: windows::core::Ref<windows::Win32::Media::Speech::ISpObjectToken>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn GetObjectToken(&self) -> windows::core::Result<windows::Win32::Media::Speech::ISpObjectToken> {
        Err(windows::core::Error::from(windows::Win32::Foundation::E_FAIL))
    }
}
