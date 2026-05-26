mod common;

use common::{CLSID_SAPIENCE_VOICE, DllHandle, HklmOverride};
use serial_test::serial;

#[test]
#[serial]
fn register_creates_keys_unregister_removes_them() {
    let _hklm = HklmOverride::new().expect("failed to set up HKLM override");
    let dll = DllHandle::load();

    let hr = unsafe { (dll.register)() };
    assert!(hr.is_ok(), "DllRegisterServer returned {hr:?}");

    // Verify CLSID entry
    let cls_path = format!(r"SOFTWARE\Classes\CLSID\{{{:?}}}", CLSID_SAPIENCE_VOICE);
    let cls = windows_registry::LOCAL_MACHINE
        .open(&cls_path)
        .expect("CLSID key missing");
    let _ = cls
        .open("InprocServer32")
        .expect("InprocServer32 key missing");

    // Verify voice token
    let token = windows_registry::LOCAL_MACHINE
        .open(r"SOFTWARE\Microsoft\Speech\Voices\Tokens\SAPIence")
        .expect("voice token key missing");
    let attrs = token.open("Attributes").expect("Attributes key missing");
    let name: String = attrs.get_string("Name").expect("Name value missing");
    assert_eq!(name, "SAPIence");

    let hr = unsafe { (dll.unregister)() };
    assert!(hr.is_ok(), "DllUnregisterServer returned {hr:?}");

    assert!(
        windows_registry::LOCAL_MACHINE.open(&cls_path).is_err(),
        "CLSID key still present after unregister"
    );
    assert!(
        windows_registry::LOCAL_MACHINE
            .open(r"SOFTWARE\Microsoft\Speech\Voices\Tokens\SAPIence")
            .is_err(),
        "voice token still present after unregister"
    );
}
