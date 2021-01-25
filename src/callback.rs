use std::{ffi::c_void, panic};

use winapi::um::winnt::{PTP_CALLBACK_INSTANCE, PTP_WORK};

use async_task::Runnable;

use crate::threadpool::Handle;

pub struct CallbackContext {
    pub(crate) runnable: Runnable,
    pub(crate) handle: Handle,
}

pub(crate) unsafe extern "system" fn callback(
    instance: PTP_CALLBACK_INSTANCE,
    context: *mut c_void,
    _work: PTP_WORK,
) {
    let CallbackContext {
        runnable,
        mut handle,
    } = *Box::from_raw(context as *mut CallbackContext);

    handle.set_callback_instance(instance);
    let _enter = handle.enter();

    panic::catch_unwind(move || runnable.run()).ok();
}
