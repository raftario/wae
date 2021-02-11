use std::{cmp::Ordering, ffi::c_void, fmt, io, mem, ops::Deref, ptr, sync::Arc};

use winapi::{
    shared::minwindef::{FALSE, TRUE},
    um::{
        sysinfoapi::{GetSystemInfo, SYSTEM_INFO},
        threadpoolapiset::{
            CloseThreadpool, CloseThreadpoolCleanupGroup, CloseThreadpoolCleanupGroupMembers,
            CreateThreadpool, CreateThreadpoolCleanupGroup, CreateThreadpoolWork,
            SetThreadpoolThreadMaximum, SetThreadpoolThreadMinimum, SubmitThreadpoolWork,
        },
        winnt::{
            TP_CALLBACK_ENVIRON_V3_u, PTP_CALLBACK_INSTANCE, PTP_WORK, TP_CALLBACK_ENVIRON_V3,
            TP_CALLBACK_PRIORITY_HIGH, TP_CALLBACK_PRIORITY_LOW, TP_CALLBACK_PRIORITY_NORMAL,
        },
    },
};

use async_task::Runnable;
use crossbeam_queue::SegQueue;

pub use crate::context::ContextGuard;

#[derive(Debug)]
pub struct Threadpool {
    handle: Handle,
}

#[derive(Clone)]
pub struct Handle {
    inner: Arc<HandleInner>,
    priority: Priority,
    pub(crate) callback_instance: Option<PTP_CALLBACK_INSTANCE>,
    #[cfg(feature = "tracing")]
    pub(crate) span: Option<tracing::Span>,
}

unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

#[derive(Debug, Clone, Copy)]
pub struct Builder {
    max_threads: u32,
    min_threads: u32,
    #[cfg(feature = "net")]
    net: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum Priority {
    High = TP_CALLBACK_PRIORITY_HIGH,
    Normal = TP_CALLBACK_PRIORITY_NORMAL,
    Low = TP_CALLBACK_PRIORITY_LOW,
}

struct HandleInner {
    high_queue: TaskQueue,
    normal_queue: TaskQueue,
    low_queue: TaskQueue,
    callback_environ: TP_CALLBACK_ENVIRON_V3,
}

unsafe impl Send for HandleInner {}
unsafe impl Sync for HandleInner {}

struct TaskQueue {
    queue: SegQueue<(Runnable, Handle)>,
    work: PTP_WORK,
}

impl Threadpool {
    pub fn new() -> io::Result<Threadpool> {
        Builder::default().build()
    }

    pub fn builder() -> Builder {
        Builder::default()
    }
}

impl Handle {
    pub(crate) fn push_task(&self, runnable: Runnable) {
        let queue = match self.priority {
            Priority::High => &self.inner.high_queue,
            Priority::Normal => &self.inner.normal_queue,
            Priority::Low => &self.inner.low_queue,
        };
        queue.queue.push((runnable, self.clone()));
        unsafe {
            SubmitThreadpoolWork(queue.work);
        }
    }

    #[cfg(any(feature = "net"))]
    pub(crate) fn callback_environ(&self) -> TP_CALLBACK_ENVIRON_V3 {
        let mut ce = self.inner.callback_environ;
        ce.CallbackPriority = self.priority as u32;
        ce
    }

    pub fn set_max_threads(&self, maximum: u32) -> &Self {
        unsafe { SetThreadpoolThreadMaximum(self.inner.callback_environ.Pool, maximum) }
        self
    }

    pub fn set_min_threads(&self, minimum: u32) -> &Self {
        self.try_set_min_threads(minimum).unwrap()
    }

    pub fn try_set_min_threads(&self, minimum: u32) -> io::Result<&Self> {
        if unsafe { SetThreadpoolThreadMinimum(self.inner.callback_environ.Pool, minimum) } == TRUE
        {
            Ok(self)
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub fn priority(&self) -> Priority {
        self.priority
    }

    pub fn set_priority(&mut self, priority: Priority) -> &mut Self {
        self.priority = priority;
        self
    }
}

impl Builder {
    pub fn new() -> Builder {
        let mut system_info = SYSTEM_INFO::default();
        unsafe { GetSystemInfo(&mut system_info) };

        Self {
            max_threads: 512,
            min_threads: system_info.dwNumberOfProcessors,
            #[cfg(feature = "net")]
            net: true,
        }
    }

    pub fn min_threads(mut self, max: u32) -> Builder {
        self.max_threads = max;
        self
    }

    pub fn max_threads(mut self, min: u32) -> Builder {
        self.min_threads = min;
        self
    }

    #[cfg(feature = "net")]
    pub fn net(mut self, enabled: bool) -> Builder {
        self.net = enabled;
        self
    }

    pub fn build(self) -> io::Result<Threadpool> {
        let pool = unsafe { CreateThreadpool(ptr::null_mut()) };
        if pool.is_null() {
            return Err(io::Error::last_os_error());
        }

        unsafe { SetThreadpoolThreadMaximum(pool, self.max_threads) };
        if unsafe { SetThreadpoolThreadMinimum(pool, self.min_threads) } == FALSE {
            unsafe { CloseThreadpool(pool) };
            return Err(io::Error::last_os_error());
        }

        let cleanup_group = unsafe { CreateThreadpoolCleanupGroup() };
        if cleanup_group.is_null() {
            unsafe { CloseThreadpool(pool) };
            return Err(io::Error::last_os_error());
        }

        let mut callback_environ = TP_CALLBACK_ENVIRON_V3 {
            Version: 3,
            Pool: pool,
            CleanupGroup: cleanup_group,
            CleanupGroupCancelCallback: None,
            RaceDll: ptr::null_mut(),
            ActivationContext: ptr::null_mut(),
            FinalizationCallback: None,
            u: TP_CALLBACK_ENVIRON_V3_u::default(),
            CallbackPriority: Priority::default() as u32,
            Size: mem::size_of::<TP_CALLBACK_ENVIRON_V3>() as u32,
        };

        let mut inner = Arc::new(HandleInner {
            high_queue: TaskQueue {
                queue: SegQueue::new(),
                work: ptr::null_mut(),
            },
            normal_queue: TaskQueue {
                queue: SegQueue::new(),
                work: ptr::null_mut(),
            },
            low_queue: TaskQueue {
                queue: SegQueue::new(),
                work: ptr::null_mut(),
            },
            callback_environ,
        });
        let inner_mut = Arc::get_mut(&mut inner).unwrap();

        inner_mut.high_queue.work = Self::create_work(
            Priority::High,
            &inner_mut.high_queue.queue,
            &mut callback_environ,
        )?;
        inner_mut.normal_queue.work = Self::create_work(
            Priority::Normal,
            &inner_mut.normal_queue.queue,
            &mut callback_environ,
        )?;
        inner_mut.low_queue.work = Self::create_work(
            Priority::Low,
            &inner_mut.low_queue.queue,
            &mut callback_environ,
        )?;

        #[cfg(feature = "net")]
        if self.net {
            let mut wsadata = winapi::um::winsock2::WSADATA::default();
            let ret = unsafe { winapi::um::winsock2::WSAStartup(0x0202, &mut wsadata) };
            if ret != 0 {
                unsafe { CloseThreadpool(pool) };
                return Err(io::Error::from_raw_os_error(ret));
            }
        }

        Ok(Threadpool {
            handle: Handle {
                inner,
                priority: Priority::Normal,
                callback_instance: None,
                #[cfg(feature = "tracing")]
                span: None,
            },
        })
    }

    fn create_work(
        priority: Priority,
        queue: &SegQueue<(Runnable, Handle)>,
        callback_environ: &mut TP_CALLBACK_ENVIRON_V3,
    ) -> io::Result<PTP_WORK> {
        callback_environ.CallbackPriority = priority as u32;
        let work = unsafe {
            CreateThreadpoolWork(
                Some(crate::task::callback),
                queue as *const _ as *mut c_void,
                callback_environ,
            )
        };
        if !work.is_null() {
            Ok(work)
        } else {
            unsafe {
                CloseThreadpoolCleanupGroupMembers(
                    callback_environ.CleanupGroup,
                    TRUE,
                    ptr::null_mut(),
                );
                CloseThreadpoolCleanupGroup(callback_environ.CleanupGroup);
                CloseThreadpool(callback_environ.Pool);
            }
            Err(io::Error::last_os_error())
        }
    }
}

impl Deref for Threadpool {
    type Target = Handle;

    fn deref(&self) -> &Self::Target {
        &self.handle
    }
}

impl fmt::Debug for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Handle")
            .field("pool", &self.inner.callback_environ.Pool)
            .field("priority", &self.priority)
            .finish()
    }
}

impl Drop for HandleInner {
    fn drop(&mut self) {
        unsafe {
            CloseThreadpoolCleanupGroupMembers(
                self.callback_environ.CleanupGroup,
                TRUE,
                ptr::null_mut(),
            );
            CloseThreadpoolCleanupGroup(self.callback_environ.CleanupGroup);
            CloseThreadpool(self.callback_environ.Pool);
        }
    }
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for Priority {
    fn default() -> Self {
        Self::Normal
    }
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
