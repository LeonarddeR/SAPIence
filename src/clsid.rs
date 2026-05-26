//! Compile-time constants for the SAPIence COM voice.

use windows::core::GUID;

/// CLSID of the SAPIence voice COM object.
/// Generated once via `uuidgen` and frozen.
pub const CLSID_SAPIENCE_VOICE: GUID = GUID::from_u128(0x5A91_E9CE_2BC7_4F8E_9DA1_4D7C_9F2E_7E11);

/// Display name used both in the CLSID `(Default)` and the voice token.
pub const VOICE_DISPLAY_NAME: &str = "SAPIence (NVDA)";

/// Voice token folder name under
/// HKLM\SOFTWARE\Microsoft\Speech\Voices\Tokens\<this>.
pub const VOICE_TOKEN_NAME: &str = "SAPIence";

pub const VOICE_VENDOR: &str = "Leonard de Ruijter";
pub const VOICE_LANGUAGE_LCID_HEX: &str = "409";
pub const VOICE_AGE: &str = "Adult";
pub const VOICE_GENDER: &str = "Neutral";
pub const VOICE_VERSION: &str = "1.0";
