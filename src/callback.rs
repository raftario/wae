use std::{ffi::c_void, mem::MaybeUninit, panic};

use async_task_ffi::Runnable;
use panic::RefUnwindSafe;
use winapi::um::winnt::{PTP_CALLBACK_INSTANCE, PTP_WORK};

use crate::threadpool::Handle;

pub struct CallbackContext {
    pub(crate) handle: Handle,
}

impl RefUnwindSafe for CallbackContext {}

pub(crate) unsafe extern "system" fn callback(
    instance: PTP_CALLBACK_INSTANCE,
    context: *mut c_void,
    _work: PTP_WORK,
) {
    let runnable: Runnable<MaybeUninit<CallbackContext>> = Runnable::from_raw(context as *mut ());
    let CallbackContext { mut handle } = runnable.data().as_ptr().read();

    handle.set_callback_instance(instance);
    let _context = handle.enter();
    #[cfg(feature = "tracing")]
    let _span = handle.enter_span();

    panic::catch_unwind(move || runnable.run()).ok();
}
