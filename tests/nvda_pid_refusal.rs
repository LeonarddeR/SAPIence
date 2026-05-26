mod common;

use common::{DllHandle, CLSID_SAPIENCE_VOICE};
use serial_test::serial;
use windows::Win32::Foundation::CLASS_E_CLASSNOTAVAILABLE;
use windows::Win32::System::Com::IClassFactory;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::core::Interface;
use core::ffi::c_void;

type SetPidFn = unsafe extern "Rust" fn(u32);
type ClearPidFn = unsafe extern "Rust" fn();

#[test]
#[serial]
fn refuses_when_pid_matches_nvda() {
    let dll = DllHandle::load();
    let lib = unsafe { libloading::Library::new(common::dll_path()) }
        .expect("LoadLibrary failed");

    let set: libloading::Symbol<SetPidFn> = unsafe {
        lib.get(b"sapience_test_set_pid_override\0")
    }.expect("test hook missing — build with --features test-hooks");
    let clear: libloading::Symbol<ClearPidFn> = unsafe {
        lib.get(b"sapience_test_clear_pid_override\0")
    }.expect("clear hook missing");

    let me = unsafe { GetCurrentProcessId() };
    unsafe { set(me); }

    let mut factory_ptr: *mut c_void = std::ptr::null_mut();
    let hr = unsafe {
        (dll.get_class_object)(
            &CLSID_SAPIENCE_VOICE,
            &IClassFactory::IID,
            &mut factory_ptr,
        )
    };
    assert!(hr.is_ok(), "DllGetClassObject failed: {hr:?}");
    assert!(!factory_ptr.is_null());
    let factory = unsafe { IClassFactory::from_raw(factory_ptr) };

    let result = unsafe { factory.CreateInstance::<Option<_>, windows::core::IUnknown>(None) };
    unsafe { clear(); }

    let err = result.expect_err("CreateInstance should have refused (PID match)");
    assert_eq!(
        err.code(),
        CLASS_E_CLASSNOTAVAILABLE,
        "expected CLASS_E_CLASSNOTAVAILABLE, got {:?}",
        err.code()
    );
}
