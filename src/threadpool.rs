use std::{cmp::Ordering, fmt, mem, ptr};

use winapi::{
    shared::minwindef::FALSE,
    um::{
        sysinfoapi::{GetSystemInfo, SYSTEM_INFO},
        threadpoolapiset::{
            CloseThreadpool, CloseThreadpoolCleanupGroup, CloseThreadpoolCleanupGroupMembers,
            CreateThreadpool, CreateThreadpoolCleanupGroup, SetThreadpoolThreadMaximum,
            SetThreadpoolThreadMinimum,
        },
        winnt::{
            TP_CALLBACK_ENVIRON_V3_u, PTP_CALLBACK_INSTANCE, TP_CALLBACK_ENVIRON_V3,
            TP_CALLBACK_PRIORITY_HIGH, TP_CALLBACK_PRIORITY_LOW, TP_CALLBACK_PRIORITY_NORMAL,
        },
    },
};

use crate::{error::Error, sync::Once};

pub use crate::context::ContextGuard;

pub struct Threadpool {
    handle: Handle,
    close: Once,
}

impl Drop for Threadpool {
    fn drop(&mut self) {
        self.close(true)
    }
}

impl fmt::Debug for Threadpool {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("Threadpool")
            .field(&self.handle.callback_environ.Pool)
            .finish()
    }
}

impl Threadpool {
    pub fn new() -> Result<Self, Error> {
        Builder::default().build()
    }

    pub fn builder() -> Builder {
        Builder::default()
    }

    pub fn handle(&self) -> &Handle {
        &self.handle
    }

    pub fn close(&self, wait: bool) {
        self.close.call_once(|| unsafe {
            CloseThreadpoolCleanupGroupMembers(
                self.handle.callback_environ.CleanupGroup,
                (!wait).into(),
                ptr::null_mut(),
            );
            CloseThreadpoolCleanupGroup(self.handle.callback_environ.CleanupGroup);
            CloseThreadpool(self.handle.callback_environ.Pool);
        })
    }

    pub fn set_thread_maximum(&self, maximum: u32) -> &Self {
        self.handle.set_thread_maximum(maximum);
        self
    }

    pub fn set_thread_minimum(&self, minimum: u32) -> &Self {
        self.handle.set_thread_minimum(minimum);
        self
    }
}

#[derive(Clone)]
pub struct Handle {
    pub(crate) callback_environ: TP_CALLBACK_ENVIRON_V3,
    pub(crate) callback_instance: Option<PTP_CALLBACK_INSTANCE>,
    #[cfg(feature = "tracing")]
    pub(crate) span: Option<tracing::Span>,
}

unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

impl fmt::Debug for Handle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("Handle")
            .field(&self.callback_environ.Pool)
            .finish()
    }
}

impl Handle {
    pub(crate) fn set_callback_instance(&mut self, instance: PTP_CALLBACK_INSTANCE) {
        self.callback_instance = Some(instance);
    }

    pub fn set_thread_maximum(&self, maximum: u32) -> &Self {
        unsafe { SetThreadpoolThreadMaximum(self.callback_environ.Pool, maximum) }
        self
    }

    pub fn set_thread_minimum(&self, minimum: u32) -> &Self {
        self.try_set_thread_minimum(minimum).unwrap()
    }

    pub fn try_set_thread_minimum(&self, minimum: u32) -> Result<&Self, Error> {
        if unsafe { SetThreadpoolThreadMinimum(self.callback_environ.Pool, minimum) } == FALSE {
            return Err(Error::win32());
        }
        Ok(self)
    }

    pub fn set_priority(&mut self, priority: Priority) -> &mut Self {
        self.callback_environ.CallbackPriority = priority as u32;
        self
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Builder {
    thread_maximum: u32,
    thread_minimum: u32,
}

impl Default for Builder {
    fn default() -> Self {
        let mut system_info = SYSTEM_INFO::default();
        unsafe { GetSystemInfo(&mut system_info) };

        Self {
            thread_maximum: 512,
            thread_minimum: system_info.dwNumberOfProcessors,
        }
    }
}

impl Builder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn thread_maximum(mut self, max: u32) -> Self {
        self.thread_maximum = max;
        self
    }

    pub fn thread_minimum(mut self, min: u32) -> Self {
        self.thread_minimum = min;
        self
    }

    pub fn build(self) -> Result<Threadpool, Error> {
        let pool = unsafe { CreateThreadpool(ptr::null_mut()) };
        if pool.is_null() {
            return Err(Error::win32());
        }

        unsafe { SetThreadpoolThreadMaximum(pool, self.thread_maximum) };
        if unsafe { SetThreadpoolThreadMinimum(pool, self.thread_minimum) } == 0 {
            unsafe { CloseThreadpool(pool) };
            return Err(Error::win32());
        }

        let cleanup_group = unsafe { CreateThreadpoolCleanupGroup() };
        if cleanup_group.is_null() {
            unsafe { CloseThreadpool(pool) };
            return Err(Error::win32());
        }

        let callback_environ = TP_CALLBACK_ENVIRON_V3 {
            Version: 3,
            Pool: pool,
            CleanupGroup: cleanup_group,
            CleanupGroupCancelCallback: None,
            RaceDll: ptr::null_mut(),
            ActivationContext: ptr::null_mut(),
            FinalizationCallback: None,
            u: TP_CALLBACK_ENVIRON_V3_u::default(),
            CallbackPriority: Priority::Normal as u32,
            Size: mem::size_of::<TP_CALLBACK_ENVIRON_V3>() as u32,
        };

        Ok(Threadpool {
            handle: Handle {
                callback_environ,
                callback_instance: None,
                #[cfg(feature = "tracing")]
                span: None,
            },
            close: Once::new(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum Priority {
    High = TP_CALLBACK_PRIORITY_HIGH,
    Normal = TP_CALLBACK_PRIORITY_NORMAL,
    Low = TP_CALLBACK_PRIORITY_LOW,
}

macro_rules! priority_ord {
    ($self:expr, $other:expr) => {
        match ($self as u32).cmp(&($other as u32)) {
            Ordering::Less => Ordering::Greater,
            Ordering::Equal => Ordering::Equal,
            Ordering::Greater => Ordering::Less,
        }
    };
}
impl PartialOrd for Priority {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(priority_ord!(*self, *other))
    }
}
impl Ord for Priority {
    fn cmp(&self, other: &Self) -> Ordering {
        priority_ord!(*self, *other)
    }
}
