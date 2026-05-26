//! Safe Rust wrappers over the NVDA Controller Client bindings.

mod bindings {
    #![allow(
        non_upper_case_globals,
        non_camel_case_types,
        non_snake_case,
        dead_code
    )]
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

use bindings::{
    SPEECH_PRIORITY, SYMBOL_LEVEL, nvdaController_cancelSpeech, nvdaController_getProcessId,
    nvdaController_setOnSsmlMarkReachedCallback, nvdaController_speakSsml,
    nvdaController_testIfRunning,
};
pub use bindings::{error_status_t, onSsmlMarkReachedFuncType, wchar_t};
use windows::{
    Win32::Foundation::WIN32_ERROR,
    core::{HSTRING, Result},
};

#[repr(u32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SpeechPriority {
    Normal = 0,
    Next = 1,
    Now = 2,
}

#[repr(i32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SymbolLevel {
    None = 0,
    Some = 100,
    Most = 200,
    All = 300,
    Char = 1000,
    Unchanged = -1,
}

fn to_result(error: u32) -> Result<()> {
    WIN32_ERROR(error).ok()
}

pub fn test_if_running() -> Result<()> {
    to_result(unsafe { nvdaController_testIfRunning() })
}

pub fn cancel_speech() -> Result<()> {
    to_result(unsafe { nvdaController_cancelSpeech() })
}

pub fn get_process_id() -> Result<u32> {
    #[cfg(any(test, feature = "test-hooks"))]
    if let Some(v) = test_hooks::get_override() {
        return Ok(v);
    }
    let mut pid: u32 = 0;
    to_result(unsafe { nvdaController_getProcessId(&mut pid) })?;
    Ok(pid)
}

#[cfg(any(test, feature = "test-hooks"))]
pub mod test_hooks {
    use std::sync::atomic::{AtomicU32, Ordering};
    static OVERRIDE: AtomicU32 = AtomicU32::new(0);
    pub fn set_pid_override(pid: u32) {
        OVERRIDE.store(pid, Ordering::SeqCst);
    }
    pub fn clear_pid_override() {
        OVERRIDE.store(0, Ordering::SeqCst);
    }
    pub(super) fn get_override() -> Option<u32> {
        match OVERRIDE.load(Ordering::SeqCst) {
            0 => None,
            v => Some(v),
        }
    }
}

pub fn set_on_ssml_mark_reached_callback(callback: onSsmlMarkReachedFuncType) -> Result<()> {
    to_result(unsafe { nvdaController_setOnSsmlMarkReachedCallback(callback) })
}

pub fn speak_ssml(
    ssml: &str,
    symbol_level: SymbolLevel,
    priority: SpeechPriority,
    asynchronous: bool,
) -> Result<()> {
    let ssml = HSTRING::from(ssml);
    to_result(unsafe {
        nvdaController_speakSsml(
            ssml.as_ptr(),
            symbol_level as SYMBOL_LEVEL,
            priority as SPEECH_PRIORITY,
            asynchronous as bindings::boolean,
        )
    })
}
