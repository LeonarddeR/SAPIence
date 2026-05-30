//! Silent-PCM pacing, action polling, and event emission.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tracing::{trace, warn};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::Media::Speech::{
    ISpTTSEngineSite, SPEI_TTS_BOOKMARK, SPEI_WORD_BOUNDARY, SPET_LPARAM_IS_STRING, SPEVENT,
    SPEVENTENUM, SPEVENTLPARAMTYPE, SPVES_ABORT, SPVES_SKIP,
};
use windows::core::HSTRING;

use crate::marks::MarkChannel;

pub const SAMPLE_RATE_HZ: u32 = 22050;
pub const BYTES_PER_SAMPLE: u32 = 2; // 16-bit mono
pub const CHUNK_MS: u64 = 50;

pub fn chunk_bytes() -> usize {
    ((SAMPLE_RATE_HZ as u64 * BYTES_PER_SAMPLE as u64 * CHUNK_MS) / 1000) as usize
}

pub enum PaceOutcome {
    Completed,
    Aborted,
    Skip { count: i32, skip_type: i32 },
    SafetyCapped,
}

pub fn pace_until_end(
    site: &ISpTTSEngineSite,
    channel: Arc<MarkChannel>,
    interest: u64,
    audio_offset: &mut u64,
    safety_cap: Duration,
    speech_ended: &AtomicBool,
) -> PaceOutcome {
    let chunk = vec![0u8; chunk_bytes()];
    let start = Instant::now();
    loop {
        // Write a silent chunk; SAPI flow-controls naturally via Write backpressure.
        let res = unsafe {
            site.Write(
                chunk.as_ptr() as *const core::ffi::c_void,
                chunk.len() as u32,
            )
        };
        match res {
            Ok(written) => {
                *audio_offset += written as u64;
            }
            Err(e) => {
                warn!("ISpTTSEngineSite::Write failed: {e}");
                return PaceOutcome::Completed;
            }
        }

        // Poll actions.
        let actions = unsafe { site.GetActions() };
        if actions & (SPVES_ABORT.0 as u32) != 0 {
            return PaceOutcome::Aborted;
        }
        if actions & (SPVES_SKIP.0 as u32) != 0 {
            let mut t = windows::Win32::Media::Speech::SPVSKIPTYPE(0);
            let mut n: i32 = 0;
            if unsafe { site.GetSkipInfo(&mut t, &mut n) }.is_ok() {
                return PaceOutcome::Skip {
                    count: n,
                    skip_type: t.0,
                };
            }
        }

        // Drain marks and fire events.
        let snap = channel.drain();
        for w in snap.words {
            if interest & event_bit(SPEI_WORD_BOUNDARY) != 0 {
                add_word_event(site, *audio_offset, w);
            }
        }
        for b in snap.bookmarks {
            if interest & event_bit(SPEI_TTS_BOOKMARK) != 0 {
                add_bookmark_event(site, *audio_offset, &b);
            }
        }
        if snap.end_reached {
            return PaceOutcome::Completed;
        }

        // Worker exited (speak_ssml returned SUCCESS or ERROR_CANCELLED). Stop pacing.
        if speech_ended.load(Ordering::Acquire) {
            return PaceOutcome::Completed;
        }

        // Safety cap.
        if start.elapsed() >= safety_cap {
            warn!("pacing safety cap reached after {:?}", start.elapsed());
            return PaceOutcome::SafetyCapped;
        }

        trace!("paced one chunk; audio_offset={}", audio_offset);
    }
}

/// True if the client subscribed to any timed event SAPIence can actually
/// produce from NVDA marks: word boundary or bookmark. Sentence/viseme/phoneme
/// are never available from the controller client, so they are not considered.
///
/// Drives path selection in `Speak`: no timed interest → the no-`Write`
/// [`poll_until_end`] loop; timed interest → the silent-PCM [`pace_until_end`]
/// loop, whose audio stream is the only timeline SAPI has for firing events.
pub fn wants_timed_events(interest: u64) -> bool {
    interest & (event_bit(SPEI_WORD_BOUNDARY) | event_bit(SPEI_TTS_BOOKMARK)) != 0
}

/// No-`Write` pacing for clients that requested no timed events.
///
/// The caller's worker thread drives NVDA synchronously (gating `Speak`
/// duration); this loop only polls `GetActions` for abort/skip and waits for the
/// end mark, the worker exit, or `safety_cap`. It writes no PCM and fires no
/// events: with no audio stream there is no offset to synchronise against, and
/// the only path that reaches here wanted no events anyway. For a silence
/// fragment (no NVDA call, never-signalling channel) the `safety_cap` wall-clock
/// wait is the audible gap between NVDA utterances.
pub fn poll_until_end(
    site: &ISpTTSEngineSite,
    channel: Arc<MarkChannel>,
    safety_cap: Duration,
    speech_ended: &AtomicBool,
) -> PaceOutcome {
    const POLL: Duration = Duration::from_millis(20);
    let start = Instant::now();
    loop {
        // Abort/skip ride GetActions, independent of Write.
        let actions = unsafe { site.GetActions() };
        if actions & (SPVES_ABORT.0 as u32) != 0 {
            return PaceOutcome::Aborted;
        }
        if actions & (SPVES_SKIP.0 as u32) != 0 {
            let mut t = windows::Win32::Media::Speech::SPVSKIPTYPE(0);
            let mut n: i32 = 0;
            if unsafe { site.GetSkipInfo(&mut t, &mut n) }.is_ok() {
                return PaceOutcome::Skip {
                    count: n,
                    skip_type: t.0,
                };
            }
        }

        // Wait for the end mark (or any mark) without spinning, re-polling
        // actions every POLL. Drained marks are discarded: with no audio stream
        // there is no offset to fire word/bookmark events against.
        let snap = channel.wait_drain(POLL);
        if snap.end_reached {
            return PaceOutcome::Completed;
        }

        // Worker finished the synchronous NVDA call (success or cancellation).
        if speech_ended.load(Ordering::Acquire) {
            return PaceOutcome::Completed;
        }

        // Safety cap; also the timer that renders a silence fragment's gap.
        if start.elapsed() >= safety_cap {
            warn!("poll safety cap reached after {:?}", start.elapsed());
            return PaceOutcome::SafetyCapped;
        }
    }
}

fn event_bit(e: SPEVENTENUM) -> u64 {
    1u64 << (e.0 as u32)
}

fn add_word_event(site: &ISpTTSEngineSite, audio_offset: u64, word_idx: u32) {
    let ev = SPEVENT {
        _bitfield: pack_bitfield(SPEI_WORD_BOUNDARY, SPEVENTENUM(0)),
        ulStreamNum: 0,
        ullAudioStreamOffset: audio_offset,
        wParam: WPARAM(0),
        lParam: LPARAM(word_idx as isize),
    };
    let _ = unsafe { site.AddEvents(&ev, 1) };
}

fn add_bookmark_event(site: &ISpTTSEngineSite, audio_offset: u64, name: &str) {
    let hs = HSTRING::from(name);
    let ev = SPEVENT {
        _bitfield: pack_bitfield_lp(SPEI_TTS_BOOKMARK, SPET_LPARAM_IS_STRING),
        ulStreamNum: 0,
        ullAudioStreamOffset: audio_offset,
        wParam: WPARAM(0),
        lParam: LPARAM(hs.as_ptr() as isize),
    };
    let _ = unsafe { site.AddEvents(&ev, 1) };
    // hs must stay alive until AddEvents returns; drop here is after the call.
}

fn pack_bitfield(event_id: SPEVENTENUM, lparam_type: SPEVENTENUM) -> i32 {
    (event_id.0 & 0xFFFF) | ((lparam_type.0 & 0xFF) << 16)
}

fn pack_bitfield_lp(event_id: SPEVENTENUM, lparam_type: SPEVENTLPARAMTYPE) -> i32 {
    (event_id.0 & 0xFFFF) | ((lparam_type.0 & 0xFF) << 16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::marks::MarkChannel;
    use core::ffi::c_void;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use windows::Win32::Media::Speech::{
        ISpEventSink_Impl, ISpTTSEngineSite, ISpTTSEngineSite_Impl, SPEVENT, SPVSKIPTYPE,
    };
    use windows::core::{Result, implement};

    #[test]
    fn chunk_bytes_at_22050_hz_16bit_mono() {
        // 22050 Hz * 2 B/sample * 0.05 s = 2205 B
        assert_eq!(chunk_bytes(), 2205);
    }

    #[test]
    fn wants_timed_events_for_word_boundary_and_bookmark_only() {
        assert!(wants_timed_events(event_bit(SPEI_WORD_BOUNDARY)));
        assert!(wants_timed_events(event_bit(SPEI_TTS_BOOKMARK)));
        assert!(wants_timed_events(
            event_bit(SPEI_WORD_BOUNDARY) | event_bit(SPEI_TTS_BOOKMARK)
        ));
        // No interest, or interest in events we cannot produce, selects poll path.
        assert!(!wants_timed_events(0));
        assert!(!wants_timed_events(event_bit(SPEVENTENUM(8)))); // SPEI_VISEME
    }

    /// Minimal `ISpTTSEngineSite` recording `Write`/`AddEvents` calls and
    /// returning a fixed `GetActions` mask. Counters are `Arc`-shared so the
    /// test can inspect them after the COM object is consumed by `.into()`.
    #[implement(ISpTTSEngineSite)]
    struct FakeSite {
        writes: Arc<AtomicUsize>,
        add_events: Arc<AtomicUsize>,
        actions: u32,
    }

    impl ISpEventSink_Impl for FakeSite_Impl {
        fn AddEvents(&self, _p: *const SPEVENT, _c: u32) -> Result<()> {
            self.add_events.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn GetEventInterest(&self, p: *mut u64) -> Result<()> {
            unsafe { *p = 0 };
            Ok(())
        }
    }

    impl ISpTTSEngineSite_Impl for FakeSite_Impl {
        fn GetActions(&self) -> u32 {
            self.actions
        }
        fn Write(&self, _pbuff: *const c_void, cb: u32) -> Result<u32> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(cb)
        }
        fn GetRate(&self) -> Result<i32> {
            Ok(0)
        }
        fn GetVolume(&self) -> Result<u16> {
            Ok(100)
        }
        fn GetSkipInfo(&self, _t: *mut SPVSKIPTYPE, _n: *mut i32) -> Result<()> {
            Ok(())
        }
        fn CompleteSkip(&self, _n: i32) -> Result<()> {
            Ok(())
        }
    }

    fn fake_site(actions: u32) -> (ISpTTSEngineSite, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let writes = Arc::new(AtomicUsize::new(0));
        let add_events = Arc::new(AtomicUsize::new(0));
        let site: ISpTTSEngineSite = FakeSite {
            writes: writes.clone(),
            add_events: add_events.clone(),
            actions,
        }
        .into();
        (site, writes, add_events)
    }

    #[test]
    fn poll_path_writes_no_pcm_and_fires_no_events_on_speech_ended() {
        let (site, writes, add_events) = fake_site(0); // SPVES_CONTINUE
        let ch = Arc::new(MarkChannel::default());
        let ended = AtomicBool::new(true);

        let outcome = poll_until_end(&site, ch, Duration::from_secs(5), &ended);

        assert!(matches!(outcome, PaceOutcome::Completed));
        assert_eq!(
            writes.load(Ordering::SeqCst),
            0,
            "poll path must not write PCM"
        );
        assert_eq!(
            add_events.load(Ordering::SeqCst),
            0,
            "poll path must not fire events"
        );
    }

    #[test]
    fn poll_path_aborts_on_action_without_writing() {
        let (site, writes, _ae) = fake_site(SPVES_ABORT.0 as u32);
        let ch = Arc::new(MarkChannel::default());
        let ended = AtomicBool::new(false);

        let outcome = poll_until_end(&site, ch, Duration::from_secs(5), &ended);

        assert!(matches!(outcome, PaceOutcome::Aborted));
        assert_eq!(writes.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn poll_path_safety_caps_when_nothing_signals() {
        // Never-signalling channel + speech not ended → returns via cap (the
        // silence-fragment path). Cap is short so the test stays fast.
        let (site, writes, _ae) = fake_site(0);
        let ch = Arc::new(MarkChannel::default());
        let ended = AtomicBool::new(false);

        let outcome = poll_until_end(&site, ch, Duration::from_millis(30), &ended);

        assert!(matches!(outcome, PaceOutcome::SafetyCapped));
        assert_eq!(writes.load(Ordering::SeqCst), 0);
    }
}
