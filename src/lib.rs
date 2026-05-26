//! SAPIence — SAPI 5 TTS engine forwarding speech to NVDA.

#![cfg(windows)]

pub mod class_factory;
pub mod clsid;
pub mod nvda;
pub mod registry;
pub mod tts_engine;
