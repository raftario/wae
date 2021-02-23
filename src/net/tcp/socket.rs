use std::{io, ptr};

use winapi::{
    shared::{
        minwindef::TRUE,
        ws2def::{AF_UNSPEC, IPPROTO_TCP},
    },
    um::{
        ioapiset::{CancelIoEx, GetOverlappedResult},
        minwinbase::OVERLAPPED,
        winnt::HANDLE,
        winsock2::{
            closesocket, WSASocketW, INVALID_SOCKET, SOCKET, SOCK_STREAM, WSA_FLAG_OVERLAPPED,
        },
    },
};

pub(super) fn new() -> io::Result<SOCKET> {
    let socket = unsafe {
        WSASocketW(
            AF_UNSPEC,
            SOCK_STREAM,
            IPPROTO_TCP as i32,
            ptr::null_mut(),
            0,
            WSA_FLAG_OVERLAPPED,
        )
    };
    if socket != INVALID_SOCKET {
        Ok(socket)
    } else {
        Err(io::Error::last_os_error())
    }
}

pub(super) unsafe fn close(handle: HANDLE) {
    closesocket(handle as SOCKET);
}

pub(super) unsafe fn cancel(
    handle: HANDLE,
    overlapped: *mut OVERLAPPED,
    wait: bool,
) -> io::Result<()> {
    let ret = if CancelIoEx(handle, overlapped) != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    };
    if wait {
        GetOverlappedResult(handle, overlapped, &mut 0, TRUE);
    }
    ret
}
