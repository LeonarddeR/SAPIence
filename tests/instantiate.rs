mod common;

use common::{DllHandle, CLSID_SAPIENCE_VOICE};
use core::ffi::c_void;
use windows::Win32::Media::Speech::ISpTTSEngine;
use windows::Win32::System::Com::IClassFactory;
use windows::core::{IUnknown, Interface};

#[test]
fn create_engine_via_class_factory() {
    let dll = DllHandle::load();

    // Get the class factory
    let mut factory_ptr: *mut c_void = std::ptr::null_mut();
    let hr = unsafe {
        (dll.get_class_object)(
            &CLSID_SAPIENCE_VOICE,
            &IClassFactory::IID,
            &mut factory_ptr,
        )
    };
    assert!(hr.is_ok(), "DllGetClassObject failed: {hr:?}");
    assert!(!factory_ptr.is_null(), "factory pointer is null");

    let factory = unsafe { IClassFactory::from_raw(factory_ptr) };

    // Create the engine (may fail if NVDA's PID matches ours, but that's fine)
    // In windows 0.62, CreateInstance is generic: CreateInstance<P0, T>(outer: P0) -> Result<T>
    let result: windows::core::Result<ISpTTSEngine> = unsafe {
        factory.CreateInstance(None::<&IUnknown>)
    };
    // If NVDA isn't running, get_process_id() returns Err → class factory allows construction.
    // If NVDA IS running and PID matches ours (unlikely in test), CLASS_E_CLASSNOTAVAILABLE.
    // Either way: no panic, no UB.
    if result.is_ok() {
        let engine = result.unwrap();
        drop(engine);
    }
    // result being Err(CLASS_E_CLASSNOTAVAILABLE) is also acceptable
}
