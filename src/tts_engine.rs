//! ISpTTSEngine + ISpObjectWithToken implementation.

use parking_lot::Mutex;
use std::time::Duration;
use tracing::{instrument, warn};
use windows::{
    Win32::{
        Foundation::{E_FAIL, E_POINTER},
        Media::Audio::WAVEFORMATEX,
        Media::Speech::{
            ISpObjectToken, ISpObjectWithToken, ISpObjectWithToken_Impl, ISpTTSEngine,
            ISpTTSEngine_Impl, ISpTTSEngineSite, SPVA_Bookmark, SPVA_Silence, SPVA_Speak,
            SPVA_SpellOut, SPVTEXTFRAG,
        },
        System::Com::CoTaskMemAlloc,
    },
    core::{Error, GUID, Ref, Result, implement},
};

// SPDFID_WaveFormatEx — the only valid format identifier for ISpTTSEngine::GetOutputFormat.
// Not exposed by windows-0.62; defined from sapi.h.
const SPDFID_WAVE_FORMAT_EX: GUID = GUID::from_u128(0xc31adbae_527f_4ff5_a230_f62bb61ff70c);

use crate::{
    fragments::iter as iter_fragments,
    marks,
    nvda::{self, SpeechPriority, SymbolLevel},
    pacing::{self, BYTES_PER_SAMPLE, PaceOutcome, SAMPLE_RATE_HZ},
    ssml::{self, Prosody},
};

#[implement(ISpTTSEngine, ISpObjectWithToken)]
pub struct TtsEngine {
    token: Mutex<Option<ISpObjectToken>>,
}

impl TtsEngine {
    pub fn new() -> Self {
        Self {
            token: Mutex::new(None),
        }
    }
}

impl Default for TtsEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ISpObjectWithToken_Impl for TtsEngine_Impl {
    fn SetObjectToken(&self, ptoken: Ref<ISpObjectToken>) -> Result<()> {
        *self.token.lock() = ptoken.as_ref().cloned();
        Ok(())
    }

    fn GetObjectToken(&self) -> Result<ISpObjectToken> {
        self.token.lock().clone().ok_or_else(|| Error::from(E_FAIL))
    }
}

impl ISpTTSEngine_Impl for TtsEngine_Impl {
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    fn GetOutputFormat(
        &self,
        _ptargetfmtid: *const GUID,
        _ptargetwaveformatex: *const WAVEFORMATEX,
        pdesiredfmtid: *mut GUID,
        ppcomemdesiredwaveformatex: *mut *mut WAVEFORMATEX,
    ) -> Result<()> {
        if pdesiredfmtid.is_null() || ppcomemdesiredwaveformatex.is_null() {
            return Err(Error::from(E_POINTER));
        }
        let wfx =
            unsafe { CoTaskMemAlloc(std::mem::size_of::<WAVEFORMATEX>()) as *mut WAVEFORMATEX };
        if wfx.is_null() {
            return Err(Error::from(E_FAIL));
        }
        unsafe {
            (*wfx) = WAVEFORMATEX {
                wFormatTag: 1, // WAVE_FORMAT_PCM
                nChannels: 1,
                nSamplesPerSec: SAMPLE_RATE_HZ,
                nAvgBytesPerSec: SAMPLE_RATE_HZ * BYTES_PER_SAMPLE,
                nBlockAlign: BYTES_PER_SAMPLE as u16,
                wBitsPerSample: 16,
                cbSize: 0,
            };
            *pdesiredfmtid = SPDFID_WAVE_FORMAT_EX;
            *ppcomemdesiredwaveformatex = wfx;
        }
        Ok(())
    }

    #[instrument(skip_all)]
    #[allow(clippy::not_unsafe_ptr_arg_deref, non_upper_case_globals)]
    fn Speak(
        &self,
        _dwspeakflags: u32,
        _rguidformatid: *const GUID,
        _pwaveformatex: *const WAVEFORMATEX,
        ptextfraglist: *const SPVTEXTFRAG,
        poutputsite: Ref<ISpTTSEngineSite>,
    ) -> Result<()> {
        let site = poutputsite.ok()?.clone();

        if nvda::test_if_running().is_err() {
            warn!("NVDA not running; returning silent S_OK");
            return Ok(());
        }

        let mut interest: u64 = 0;
        let _ = unsafe { site.GetEventInterest(&mut interest) };
        let mut audio_offset: u64 = 0;
        let mut pending_bookmarks: Vec<String> = Vec::new();

        for frag in unsafe { iter_fragments(ptextfraglist) } {
            let state = frag.raw().State;
            match state.eAction {
                SPVA_Speak | SPVA_SpellOut => {
                    let utt = ssml::next_utterance_id();
                    let bookmarks_ref: Vec<&str> =
                        pending_bookmarks.iter().map(String::as_str).collect();

                    let text = frag.text_string();
                    let ssml_str = if state.eAction == SPVA_SpellOut {
                        // Build SpellOut SSML directly — no word splitting, no word marks.
                        // End mark is still needed for pacing.
                        let inner = ssml::xml_escape(&text);
                        let mut s = String::from(r#"<speak version="1.0" xml:lang="en-US">"#);
                        for b in &bookmarks_ref {
                            s.push_str(&format!(
                                r#"<mark name="{}"/>"#,
                                ssml::xml_escape(&ssml::bookmark_mark(utt, b))
                            ));
                        }
                        s.push_str(&format!(
                            r#"<say-as interpret-as="characters">{inner}</say-as>"#
                        ));
                        s.push_str(&format!(r#"<mark name="{}"/>"#, ssml::end_mark(utt)));
                        s.push_str("</speak>");
                        s
                    } else {
                        // Normal speak: use build_utterance_ssml for prosody + word marks.
                        let prosody = Prosody {
                            rate_adj: state.RateAdj,
                            volume: state.Volume,
                            pitch_adj: state.PitchAdj.MiddleAdj,
                        };
                        let (s, _word_count) = ssml::build_utterance_ssml(
                            utt,
                            &text,
                            "en-US",
                            prosody,
                            &bookmarks_ref,
                        );
                        s
                    };
                    pending_bookmarks.clear();

                    let ch = marks::register(utt);

                    // Spawn worker thread for synchronous NVDA call.
                    // asynchronous=false is required for mark callbacks to fire.
                    let ssml_for_worker = ssml_str.clone();
                    let worker = std::thread::spawn(move || {
                        let _ = nvda::speak_ssml(
                            &ssml_for_worker,
                            SymbolLevel::Unchanged,
                            SpeechPriority::Next,
                            false, // synchronous — required for mark callbacks
                        );
                    });

                    let cap = Duration::from_millis(200 * (text.chars().count() as u64 + 1));
                    let outcome =
                        pacing::pace_until_end(&site, ch, interest, &mut audio_offset, cap);
                    marks::unregister(utt);

                    match outcome {
                        PaceOutcome::Aborted => {
                            let _ = nvda::cancel_speech();
                            let _ = worker.join();
                            return Ok(());
                        }
                        PaceOutcome::Skip { .. } => {
                            let _ = nvda::cancel_speech();
                            let _ = worker.join();
                            let _ = unsafe { site.CompleteSkip(0) };
                            // Continue to next fragment after skip.
                        }
                        PaceOutcome::SafetyCapped => {
                            let _ = nvda::cancel_speech();
                            let _ = worker.join();
                            // Continue to next fragment.
                        }
                        PaceOutcome::Completed => {
                            let _ = worker.join();
                        }
                    }
                }
                SPVA_Silence => {
                    let ms = state.SilenceMSecs as u64;
                    let cap = Duration::from_millis(ms.max(10));
                    // Pace silent PCM for the requested duration; no NVDA call.
                    // Use a dummy channel that will never signal end_reached.
                    let utt = ssml::next_utterance_id();
                    let ch = marks::register(utt);
                    let _ = pacing::pace_until_end(&site, ch, interest, &mut audio_offset, cap);
                    marks::unregister(utt);
                }
                SPVA_Bookmark => {
                    pending_bookmarks.push(frag.text_string());
                }
                _ => {
                    // SPVA_Section, SPVA_ParseUnknownTag, SPVA_Pronounce: ignore.
                }
            }
        }
        Ok(())
    }
}
