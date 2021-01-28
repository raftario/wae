use std::{
    ffi::c_void,
    io,
    panic::{self, AssertUnwindSafe},
    ptr,
};

use winapi::{
    shared::minwindef::{FALSE, TRUE},
    um::{
        synchapi::CreateEventW,
        threadpoolapiset::{CreateThreadpoolWait, SetThreadpoolWait},
        winnt::{PTP_CALLBACK_INSTANCE, PTP_WAIT, TP_CALLBACK_ENVIRON_V3, TP_WAIT_RESULT},
        winsock2::{
            WSACreateEvent, WSAEnumNetworkEvents, WSAEventSelect, WSAWaitForMultipleEvents,
            FD_READ, FD_WRITE, SOCKET, WSANETWORKEVENTS, WSA_INFINITE, WSA_INVALID_EVENT,
        },
    },
};

use super::TcpStreamInner;

pub(super) unsafe fn evented(
    socket: SOCKET,
    mut callback_environ: TP_CALLBACK_ENVIRON_V3,
    inner: &mut TcpStreamInner,
) -> io::Result<()> {
    let event = CreateEventW(ptr::null_mut(), FALSE, FALSE, ptr::null_mut());
    if event == WSA_INVALID_EVENT {
        return Err(io::Error::last_os_error());
    }
    inner.event = event;
    inner.cleanup += 1;

    let wait = CreateThreadpoolWait(
        Some(callback),
        &*inner as *const TcpStreamInner as *mut c_void,
        &mut callback_environ,
    );
    if wait.is_null() {
        return Err(io::Error::last_os_error());
    }
    inner.wait = wait;
    inner.cleanup += 1;
    SetThreadpoolWait(wait, event, ptr::null_mut());

    Ok(())
}

unsafe extern "system" fn callback(
    _instance: PTP_CALLBACK_INSTANCE,
    context: *mut c_void,
    _wait: PTP_WAIT,
    _wait_result: TP_WAIT_RESULT,
) {
    dbg!("trigger");

    let context = context as *const TcpStreamInner;
    let inner = &*context;

    WSAEnumNetworkEvents(inner.socket, inner.event, &mut WSANETWORKEVENTS::default());

    panic::catch_unwind(AssertUnwindSafe(|| {
        if let Some(waker) = &*inner.read_waker.lock() {
            dbg!("gotread");
            waker.wake_by_ref();
        }
        if let Some(waker) = &*inner.write_waker.lock() {
            dbg!("gotwrite");
            waker.wake_by_ref();
        }
    }))
    .ok();
}
