#![allow(dead_code)]

use std::{io, path::PathBuf, sync::OnceLock};
use libloading::Library;
use tempfile::NamedTempFile;
use windows::{
    Win32::{
        Foundation::ERROR_SUCCESS,
        System::{
            Com::IClassFactory,
            Registry::{
                HKEY, HKEY_LOCAL_MACHINE, KEY_ALL_ACCESS, RegLoadAppKeyW, RegOverridePredefKey,
            },
        },
    },
    core::{GUID, HRESULT, Interface, OutRef, Ref, PCWSTR},
};

pub const CLSID_SAPIENCE_VOICE: GUID =
    GUID::from_u128(0x5A91_E9CE_2BC7_4F8E_9DA1_4D7C_9F2E_7E11);

pub fn dll_path() -> PathBuf {
    if let Some(p) = std::env::var_os("SAPIENCE_DLL_PATH") {
        return PathBuf::from(p);
    }
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"));
    let profile = if cfg!(debug_assertions) { "debug" } else { "release" };
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

/// RAII override of HKEY_LOCAL_MACHINE to a private in-memory hive.
pub struct HklmOverride {
    _hive: windows_registry::Key,
    _file: NamedTempFile,
}

impl HklmOverride {
    pub fn new() -> io::Result<Self> {
        let temp = tempfile::Builder::new()
            .prefix("sapience_test_hive_")
            .suffix(".dat")
            .tempfile()?;
        let path = temp.path().to_owned();
        // RegLoadAppKeyW requires the file to not exist yet.
        std::fs::remove_file(&path)?;
        let path_w: Vec<u16> = path
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0u16))
            .collect();
        let mut raw_hive = HKEY(std::ptr::null_mut());
        let rc = unsafe {
            RegLoadAppKeyW(
                PCWSTR(path_w.as_ptr()),
                &mut raw_hive,
                KEY_ALL_ACCESS.0,
                0,
                None,
            )
        };
        if rc != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(rc.0 as i32));
        }
        let hive = unsafe { windows_registry::Key::from_raw(raw_hive.0) };
        let rc2 = unsafe { RegOverridePredefKey(HKEY_LOCAL_MACHINE, Some(raw_hive)) };
        if rc2 != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(rc2.0 as i32));
        }
        Ok(Self { _hive: hive, _file: temp })
    }
}

impl Drop for HklmOverride {
    fn drop(&mut self) {
        unsafe {
            let _ = RegOverridePredefKey(HKEY_LOCAL_MACHINE, None);
        }
    }
}

pub fn read_log_tail() -> String {
    let path = std::env::temp_dir().join("SAPIence.log");
    match std::fs::read_to_string(&path) {
        Ok(s) => {
            s.lines()
                .rev()
                .take(80)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n")
        }
        Err(_) => String::new(),
    }
}
