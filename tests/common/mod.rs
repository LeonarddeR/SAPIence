#![allow(dead_code)]

use libloading::Library;
use std::{io, path::PathBuf, sync::OnceLock};
use windows::{
    Win32::{
        Foundation::ERROR_SUCCESS,
        System::Registry::{
            HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RegCloseKey, RegCreateKeyW,
            RegDeleteTreeW, RegOverridePredefKey,
        },
    },
    core::{GUID, HRESULT, PCWSTR},
};

pub const CLSID_SAPIENCE_VOICE: GUID = GUID::from_u128(0x5A91_E9CE_2BC7_4F8E_9DA1_4D7C_9F2E_7E11);

pub fn dll_path() -> PathBuf {
    if let Some(p) = std::env::var_os("SAPIENCE_DLL_PATH") {
        return PathBuf::from(p);
    }
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"));
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let triple = if cfg!(target_arch = "aarch64") {
        "aarch64-pc-windows-msvc"
    } else if cfg!(target_arch = "x86_64") {
        "x86_64-pc-windows-msvc"
    } else if cfg!(target_arch = "x86") {
        "i686-pc-windows-msvc"
    } else {
        ""
    };
    if !triple.is_empty() {
        let p = target_dir.join(triple).join(profile).join("sapience.dll");
        if p.is_file() {
            return p;
        }
    }
    target_dir.join(profile).join("sapience.dll")
}

pub type DllGetClassObjectFn = unsafe extern "system" fn(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut core::ffi::c_void,
) -> HRESULT;

pub type DllRegisterServerFn = unsafe extern "system" fn() -> HRESULT;
pub type DllUnregisterServerFn = unsafe extern "system" fn() -> HRESULT;

pub struct DllHandle {
    pub get_class_object: libloading::Symbol<'static, DllGetClassObjectFn>,
    pub register: libloading::Symbol<'static, DllRegisterServerFn>,
    pub unregister: libloading::Symbol<'static, DllUnregisterServerFn>,
    _lib: &'static Library,
}

unsafe impl Send for DllHandle {}
unsafe impl Sync for DllHandle {}

impl DllHandle {
    pub fn load() -> &'static DllHandle {
        static HANDLE: OnceLock<DllHandle> = OnceLock::new();
        HANDLE.get_or_init(|| {
            let path = dll_path();
            let lib = unsafe { Library::new(&path) }
                .unwrap_or_else(|e| panic!("LoadLibrary {path:?} failed: {e}"));
            let lib: &'static Library = Box::leak(Box::new(lib));
            unsafe {
                DllHandle {
                    get_class_object: lib.get(b"DllGetClassObject\0").unwrap(),
                    register: lib.get(b"DllRegisterServer\0").unwrap(),
                    unregister: lib.get(b"DllUnregisterServer\0").unwrap(),
                    _lib: lib,
                }
            }
        })
    }
}

/// RAII override of HKEY_LOCAL_MACHINE to a private subkey under HKCU.
///
/// Uses a regular (non-app-key) hive so that KTM transactions inside
/// `registry::register` work correctly. HKCU is always writable without
/// elevation and its hive supports transacted registry operations.
pub struct HklmOverride {
    raw_key: HKEY,
    subkey_path: Vec<u16>, // NUL-terminated UTF-16
}

impl HklmOverride {
    pub fn new() -> io::Result<Self> {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let unique = format!(r"Software\SAPIence_Test_{}_{}", std::process::id(), nanos);
        let path_w: Vec<u16> = unique.encode_utf16().chain(std::iter::once(0u16)).collect();

        let mut raw_key = HKEY(std::ptr::null_mut());
        let rc = unsafe { RegCreateKeyW(HKEY_CURRENT_USER, PCWSTR(path_w.as_ptr()), &mut raw_key) };
        if rc != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(rc.0 as i32));
        }

        let rc2 = unsafe { RegOverridePredefKey(HKEY_LOCAL_MACHINE, Some(raw_key)) };
        if rc2 != ERROR_SUCCESS {
            unsafe {
                let _ = RegCloseKey(raw_key);
            };
            return Err(io::Error::from_raw_os_error(rc2.0 as i32));
        }

        Ok(Self {
            raw_key,
            subkey_path: path_w,
        })
    }
}

impl Drop for HklmOverride {
    fn drop(&mut self) {
        unsafe {
            let _ = RegOverridePredefKey(HKEY_LOCAL_MACHINE, None);
            let _ = RegCloseKey(self.raw_key);
            // Delete the temporary subkey tree from HKCU
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(self.subkey_path.as_ptr()));
        }
    }
}

pub fn read_log_tail() -> String {
    let path = std::env::temp_dir().join("SAPIence.log");
    match std::fs::read_to_string(&path) {
        Ok(s) => s
            .lines()
            .rev()
            .take(80)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n"),
        Err(_) => String::new(),
    }
}
