//! SAPIence — SAPI 5 TTS engine forwarding speech to NVDA.

#![cfg(windows)]

pub mod class_factory;
pub mod clsid;
pub mod fragments;
pub mod marks;
pub mod nvda;
pub mod pacing;
pub mod registry;
pub mod ssml;
pub mod tts_engine;

use core::ffi::c_void;
use std::panic;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use std::sync::Once;
use tracing::{debug, error, instrument, trace, warn};
use windows::{
    Win32::{
        Foundation::{
            CLASS_E_CLASSNOTAVAILABLE, E_UNEXPECTED, HMODULE, S_FALSE, S_OK,
        },
        System::{
            Com::IClassFactory,
            LibraryLoader::{
                DisableThreadLibraryCalls, GetModuleFileNameW, LoadLibraryExW,
                LOAD_WITH_ALTERED_SEARCH_PATH,
            },
            SystemServices::{DLL_PROCESS_ATTACH, DLL_PROCESS_DETACH},
        },
    },
    core::{HRESULT, Interface, OutRef, PCWSTR, Ref, GUID, BOOL, HSTRING},
};
use windows_registry::{CURRENT_USER, LOCAL_MACHINE};

use crate::{class_factory::ClassFactory, clsid::CLSID_SAPIENCE_VOICE};

pub(crate) static INSTANCE: AtomicIsize = AtomicIsize::new(0);
pub(crate) static OBJECT_COUNT: AtomicU32 = AtomicU32::new(0);

fn init_logging() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let appender = tracing_appender::rolling::never(std::env::temp_dir(), "SAPIence.log");
        let level = read_log_level().unwrap_or(tracing::Level::WARN);
        let _ = tracing_subscriber::fmt()
            .compact()
            .with_writer(appender)
            .with_ansi(false)
            .with_max_level(level)
            .try_init();
        panic::set_hook(Box::new(|info| error!("{info:?}")));
    });
}

fn read_log_level() -> Option<tracing::Level> {
    use std::str::FromStr;
    const PATH: &str = r"Software\SAPIence";
    const VALUE: &str = "LogLevel";
    let v = CURRENT_USER
        .open(PATH)
        .and_then(|k| k.get_string(VALUE))
        .or_else(|_| LOCAL_MACHINE.open(PATH).and_then(|k| k.get_string(VALUE)))
        .ok()?;
    tracing::Level::from_str(&v).ok()
}

fn preload_controller_client(hinst: HMODULE) {
    let mut buf = [0u16; 1024];
    let len = unsafe { GetModuleFileNameW(Some(hinst), &mut buf) } as usize;
    if len == 0 || len >= buf.len() {
        warn!("GetModuleFileNameW failed; cannot preload controller client");
        return;
    }
    let mut path = String::from_utf16_lossy(&buf[..len]);
    if let Some(idx) = path.rfind('\\') {
        path.truncate(idx + 1);
    }
    path.push_str("nvdaControllerClient.dll");
    let wide = HSTRING::from(path.as_str());
    let res = unsafe { LoadLibraryExW(PCWSTR(wide.as_ptr()), None, LOAD_WITH_ALTERED_SEARCH_PATH) };
    match res {
        Ok(_) => trace!("preloaded nvdaControllerClient.dll from {path}"),
        Err(e) => warn!("failed to preload nvdaControllerClient.dll: {e}"),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn DllMain(hinst: HMODULE, reason: u32, _reserved: *mut c_void) -> BOOL {
    match reason {
        DLL_PROCESS_ATTACH => {
            INSTANCE.store(hinst.0 as _, Ordering::Release);
            init_logging();
            let _ = unsafe { DisableThreadLibraryCalls(hinst) };
            preload_controller_client(hinst);
        }
        DLL_PROCESS_DETACH => {
            debug!("DllMain: DLL_PROCESS_DETACH");
        }
        _ => {}
    }
    true.into()
}

#[unsafe(no_mangle)]
#[instrument(skip_all)]
pub extern "system" fn DllGetClassObject(
    rclsid: Ref<GUID>,
    riid: Ref<GUID>,
    ppv: OutRef<IClassFactory>,
) -> HRESULT {
    let clsid = match rclsid.ok() {
        Ok(c) => *c,
        Err(e) => return e.into(),
    };
    let iid = match riid.ok() {
        Ok(i) => *i,
        Err(e) => return e.into(),
    };
    if clsid != CLSID_SAPIENCE_VOICE {
        let _ = ppv.write(None);
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    if iid != IClassFactory::IID {
        let _ = ppv.write(None);
        return E_UNEXPECTED;
    }
    let factory: IClassFactory = ClassFactory.into();
    ppv.write(Some(factory)).into()
}

#[unsafe(no_mangle)]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    if OBJECT_COUNT.load(Ordering::Acquire) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}

fn dll_path() -> Option<String> {
    let mut buf = [0u16; 1024];
    let instance = HMODULE(INSTANCE.load(Ordering::Acquire) as _);
    let len = unsafe { GetModuleFileNameW(Some(instance), &mut buf) } as usize;
    if len == 0 || len >= buf.len() {
        return None;
    }
    Some(String::from_utf16_lossy(&buf[..len]))
}

#[unsafe(no_mangle)]
#[instrument]
pub extern "system" fn DllRegisterServer() -> HRESULT {
    let Some(path) = dll_path() else {
        error!("DllRegisterServer: cannot resolve DLL path");
        return E_UNEXPECTED;
    };
    match registry::register(&LOCAL_MACHINE, &path) {
        Ok(()) => S_OK,
        Err(e) => {
            error!("DllRegisterServer failed: {e}");
            e.into()
        }
    }
}

#[unsafe(no_mangle)]
#[instrument]
pub extern "system" fn DllUnregisterServer() -> HRESULT {
    match registry::unregister(&LOCAL_MACHINE) {
        Ok(()) => S_OK,
        Err(e) => {
            error!("DllUnregisterServer failed: {e}");
            e.into()
        }
    }
}

#[cfg(feature = "test-hooks")]
#[unsafe(no_mangle)]
pub extern "Rust" fn sapience_test_set_pid_override(pid: u32) {
    nvda::test_hooks::set_pid_override(pid);
}

#[cfg(feature = "test-hooks")]
#[unsafe(no_mangle)]
pub extern "Rust" fn sapience_test_clear_pid_override() {
    nvda::test_hooks::clear_pid_override();
}
