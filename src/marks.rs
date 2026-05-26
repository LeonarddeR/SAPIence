//! Process-global SSML-mark callback dispatcher.

use parking_lot::{Condvar, Mutex, RwLock};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Once};
use tracing::{trace, warn};

use crate::nvda::{self, error_status_t, wchar_t};
use crate::ssml::{parse_mark, ParsedMark};

#[derive(Default)]
struct ChannelInner {
    new_words: Vec<u32>,
    new_bookmarks: Vec<String>,
    end_reached: bool,
}

#[derive(Default)]
pub struct MarkChannel {
    inner: Mutex<ChannelInner>,
    cv: Condvar,
}

pub struct DrainSnapshot {
    pub words: Vec<u32>,
    pub bookmarks: Vec<String>,
    pub end_reached: bool,
}

impl MarkChannel {
    pub fn drain(&self) -> DrainSnapshot {
        let mut g = self.inner.lock();
        DrainSnapshot {
            words: std::mem::take(&mut g.new_words),
            bookmarks: std::mem::take(&mut g.new_bookmarks),
            end_reached: g.end_reached,
        }
    }
}

static REGISTRY: LazyLock<RwLock<HashMap<u64, Arc<MarkChannel>>>> =
    LazyLock::new(Default::default);

pub fn register(utt: u64) -> Arc<MarkChannel> {
    install_callback();
    let ch = Arc::new(MarkChannel::default());
    REGISTRY.write().insert(utt, ch.clone());
    ch
}

pub fn unregister(utt: u64) {
    REGISTRY.write().remove(&utt);
}

unsafe extern "C" fn on_mark(name: *const wchar_t) -> error_status_t {
    if name.is_null() {
        return 0;
    }
    let mut len = 0usize;
    while unsafe { *name.add(len) } != 0 {
        len += 1;
        if len > 4096 {
            warn!("on_mark: name too long, refusing");
            return 0;
        }
    }
    let slice = unsafe { std::slice::from_raw_parts(name, len) };
    let s = String::from_utf16_lossy(slice);
    let Some(parsed) = parse_mark(&s) else {
        trace!("on_mark: unrecognised mark {s}");
        return 0;
    };
    let utt = match &parsed {
        ParsedMark::Word { utt, .. } => *utt,
        ParsedMark::End { utt } => *utt,
        ParsedMark::Bookmark { utt, .. } => *utt,
    };
    let ch = match REGISTRY.read().get(&utt) {
        Some(c) => c.clone(),
        None => return 0,
    };
    let mut g = ch.inner.lock();
    match parsed {
        ParsedMark::Word { idx, .. } => g.new_words.push(idx),
        ParsedMark::Bookmark { name, .. } => g.new_bookmarks.push(name),
        ParsedMark::End { .. } => g.end_reached = true,
    }
    ch.cv.notify_all();
    0
}

fn install_callback() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        if let Err(e) = nvda::set_on_ssml_mark_reached_callback(Some(on_mark)) {
            warn!("failed to install SSML mark callback: {e}");
        }
    });
}
