//! Registry layout for the SAPIence COM CLSID + SAPI voice token.

use crate::clsid::{
    CLSID_SAPIENCE_VOICE, VOICE_AGE, VOICE_DISPLAY_NAME, VOICE_GENDER, VOICE_LANGUAGE_LCID_HEX,
    VOICE_TOKEN_NAME, VOICE_VENDOR, VOICE_VERSION,
};
use windows::core::Result;
use windows_registry::{Key, Transaction};

pub const COM_CLS_FOLDER: &str = r"SOFTWARE\Classes\CLSID";
pub const VOICES_TOKENS_FOLDER: &str = r"SOFTWARE\Microsoft\Speech\Voices\Tokens";

fn clsid_key_path() -> String {
    format!(r"{}\{{{:?}}}", COM_CLS_FOLDER, CLSID_SAPIENCE_VOICE)
}

fn voice_token_path() -> String {
    format!(r"{}\{}", VOICES_TOKENS_FOLDER, VOICE_TOKEN_NAME)
}

pub fn register(parent: &Key, dll_path: &str) -> Result<()> {
    let t = Transaction::new()?;

    // COM CLSID entry.
    let clsid = parent
        .options()
        .write()
        .create()
        .transaction(&t)
        .open(&clsid_key_path())?;
    clsid.set_string("", VOICE_DISPLAY_NAME)?;
    let inproc = clsid
        .options()
        .write()
        .create()
        .transaction(&t)
        .open("InprocServer32")?;
    inproc.set_string("", dll_path)?;
    inproc.set_string("ThreadingModel", "Both")?;

    // SAPI voice token.
    let token = parent
        .options()
        .write()
        .create()
        .transaction(&t)
        .open(&voice_token_path())?;
    token.set_string("", VOICE_DISPLAY_NAME)?;
    token.set_string("CLSID", &format!("{{{:?}}}", CLSID_SAPIENCE_VOICE))?;
    token.set_string("LangDataPath", "")?;
    token.set_string("VoiceDataPath", "")?;
    let attrs = token
        .options()
        .write()
        .create()
        .transaction(&t)
        .open("Attributes")?;
    attrs.set_string("Name", VOICE_TOKEN_NAME)?;
    attrs.set_string("Vendor", VOICE_VENDOR)?;
    attrs.set_string("Age", VOICE_AGE)?;
    attrs.set_string("Gender", VOICE_GENDER)?;
    attrs.set_string("Language", VOICE_LANGUAGE_LCID_HEX)?;
    attrs.set_string("Version", VOICE_VERSION)?;

    t.commit()
}

pub fn unregister(parent: &Key) -> Result<()> {
    let voices = parent
        .options()
        .write()
        .read()
        .open(VOICES_TOKENS_FOLDER)?;
    let _ = voices.remove_tree(VOICE_TOKEN_NAME);

    let cls = parent.options().write().read().open(COM_CLS_FOLDER)?;
    let _ = cls.remove_tree(&format!("{{{:?}}}", CLSID_SAPIENCE_VOICE));
    Ok(())
}
