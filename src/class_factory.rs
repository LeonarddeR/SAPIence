//! IClassFactory impl that refuses to construct when running inside the
//! NVDA process (loop prevention).

use core::{ffi::c_void, mem::transmute, ptr::null_mut};
use tracing::{debug, trace, warn};
use windows::{
    Win32::{
        Foundation::{CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_NOINTERFACE},
        Media::Speech::{ISpObjectWithToken, ISpTTSEngine},
        System::{Com::{IClassFactory, IClassFactory_Impl}, Threading::GetCurrentProcessId},
    },
    core::{implement, BOOL, Error, GUID, IUnknown, Interface, Ref, Result},
};

use crate::{nvda, tts_engine::TtsEngine};

#[implement(IClassFactory)]
pub struct ClassFactory;

impl IClassFactory_Impl for ClassFactory_Impl {
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    fn CreateInstance(
        &self,
        outer: Ref<'_, IUnknown>,
        iid: *const GUID,
        object: *mut *mut c_void,
    ) -> Result<()> {
        let riid = unsafe { *iid };
        let robject = unsafe { &mut *object };
        *robject = null_mut();
        trace!("CreateInstance requested for {:?}", riid);

        if outer.is_some() {
            return Err(Error::from(CLASS_E_NOAGGREGATION));
        }

        // Loop prevention: refuse when our process IS NVDA.
        match nvda::get_process_id() {
            Ok(nvda_pid) if nvda_pid == unsafe { GetCurrentProcessId() } => {
                warn!("Refusing CreateInstance: caller process IS NVDA");
                return Err(Error::from(CLASS_E_CLASSNOTAVAILABLE));
            }
            _ => {}
        }

        let engine = TtsEngine::new();
        match riid {
            IUnknown::IID => {
                let iface: IUnknown = engine.into();
                *robject = unsafe { transmute::<IUnknown, *mut c_void>(iface) };
            }
            ISpTTSEngine::IID => {
                let iface: ISpTTSEngine = engine.into();
                *robject = unsafe { transmute::<ISpTTSEngine, *mut c_void>(iface) };
            }
            ISpObjectWithToken::IID => {
                let iface: ISpObjectWithToken = engine.into();
                *robject = unsafe { transmute::<ISpObjectWithToken, *mut c_void>(iface) };
            }
            _ => {
                debug!("Unsupported interface {:?}", riid);
                return Err(Error::from(E_NOINTERFACE));
            }
        }
        Ok(())
    }

    fn LockServer(&self, _lock: BOOL) -> Result<()> {
        Ok(())
    }
}
