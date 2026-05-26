//! Safe Rust wrappers over the NVDA Controller Client bindings.

#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case, dead_code)]

mod bindings {
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

pub use bindings::{error_status_t, onSsmlMarkReachedFuncType, wchar_t};
use bindings::{
    nvdaController_cancelSpeech, nvdaController_getProcessId,
    nvdaController_setOnSsmlMarkReachedCallback, nvdaController_speakSsml,
    nvdaController_testIfRunning, SPEECH_PRIORITY, SYMBOL_LEVEL,
};
use windows::{
    core::{HSTRING, Result},
    Win32::Foundation::WIN32_ERROR,
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
    let mut pid: u32 = 0;
    to_result(unsafe { nvdaController_getProcessId(&mut pid) })?;
    Ok(pid)
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
