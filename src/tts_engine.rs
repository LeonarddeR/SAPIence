//! ISpTTSEngine + ISpObjectWithToken implementation.

use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
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

                    // Spawn worker for the synchronous NVDA call.
                    // asynchronous=false is required: with async=true NVDA sets markCallable=None
                    // and never fires mark callbacks, so end_reached never arrives.
                    //
                    // OBJECT_COUNT is incremented here and decremented inside the worker after
                    // speak_ssml returns. This keeps DllCanUnloadNow returning S_FALSE for as
                    // long as the worker is alive, preventing a use-after-free if SAPI unloads
                    // the DLL while the worker is still blocked in the IPC call.
                    crate::OBJECT_COUNT.fetch_add(1, Ordering::Relaxed);
                    let speech_ended = Arc::new(AtomicBool::new(false));
                    let speech_ended_for_worker = Arc::clone(&speech_ended);
                    let (done_tx, done_rx) = mpsc::channel::<()>();
                    let ssml_for_worker = ssml_str.clone();
                    let worker = std::thread::spawn(move || {
                        let _ = nvda::speak_ssml(
                            &ssml_for_worker,
                            SymbolLevel::Unchanged,
                            SpeechPriority::Now,
                            false, // synchronous — required for mark callbacks
                        );
                        // Signal pacing loop to stop regardless of success or ERROR_CANCELLED.
                        speech_ended_for_worker.store(true, Ordering::Release);
                        crate::OBJECT_COUNT.fetch_sub(1, Ordering::Relaxed);
                        let _ = done_tx.send(());
                    });

                    let cap = Duration::from_millis(200 * (text.chars().count() as u64 + 1));
                    let outcome = pacing::pace_until_end(
                        &site,
                        ch,
                        interest,
                        &mut audio_offset,
                        cap,
                        &speech_ended,
                    );
                    marks::unregister(utt);

                    match outcome {
                        PaceOutcome::Aborted => {
                            cancel_and_join(worker, done_rx);
                            return Ok(());
                        }
                        PaceOutcome::Skip { .. } => {
                            cancel_and_join(worker, done_rx);
                            let _ = unsafe { site.CompleteSkip(0) };
                            // Continue to next fragment after skip.
                        }
                        PaceOutcome::SafetyCapped => {
                            cancel_and_join(worker, done_rx);
                            // Continue to next fragment.
                        }
                        PaceOutcome::Completed => {
                            // End mark fires just before synthDoneSpeaking puts None in
                            // NVDA's markQueue, so the IPC call returns imminently.
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
                    let never_ended = AtomicBool::new(false);
                    let _ = pacing::pace_until_end(
                        &site,
                        ch,
                        interest,
                        &mut audio_offset,
                        cap,
                        &never_ended,
                    );
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

/// Cancel NVDA speech then wait up to 2 s for the worker to finish.
///
/// With `asynchronous=false`, `nvdaController_speakSsml` blocks in NVDA's Python until
/// `synthDoneSpeaking` or `speechCanceled` fires. `cancel_speech` queues
/// `speech.cancelSpeech` on NVDA's event queue; once processed it fires `speechCanceled`
/// which unblocks the IPC call and lets the worker exit.
///
/// If NVDA is wedged and the worker doesn't exit within 2 s, the `JoinHandle` is
/// forgotten (thread keeps running). `OBJECT_COUNT` was incremented before spawn and
/// will be decremented by the worker when it eventually finishes, so `DllCanUnloadNow`
/// keeps returning `S_FALSE` until then.
fn cancel_and_join(worker: std::thread::JoinHandle<()>, done_rx: mpsc::Receiver<()>) {
    let _ = nvda::cancel_speech();
    match done_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(()) => {
            let _ = worker.join();
        }
        Err(_) => {
            warn!(
                "NVDA worker did not exit 2 s after cancel; leaking thread (OBJECT_COUNT holds DLL pin)"
            );
            std::mem::forget(worker);
        }
    }
}
