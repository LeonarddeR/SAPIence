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

    #[test]
    fn chunk_bytes_at_22050_hz_16bit_mono() {
        // 22050 Hz * 2 B/sample * 0.05 s = 2205 B
        assert_eq!(chunk_bytes(), 2205);
    }
}
