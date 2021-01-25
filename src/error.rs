use std::{fmt, ptr, slice};

use winapi::{
    shared::minwindef::DWORD,
    um::{
        errhandlingapi::GetLastError,
        winbase::{
            FormatMessageA, FORMAT_MESSAGE_ALLOCATE_BUFFER, FORMAT_MESSAGE_FROM_SYSTEM,
            FORMAT_MESSAGE_IGNORE_INSERTS,
        },
    },
};

#[derive(Debug)]
pub enum Error {
    Win32(DWORD),
    NoContext,
    RecursiveContext,
    RecursiveBlockOn,
    Unexpected(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Win32(code) => unsafe {
                let mut buffer = ptr::null();
                let len = FormatMessageA(
                    FORMAT_MESSAGE_ALLOCATE_BUFFER
                        | FORMAT_MESSAGE_FROM_SYSTEM
                        | FORMAT_MESSAGE_IGNORE_INSERTS,
                    ptr::null_mut(),
                    *code,
                    0x0000_0400,
                    &mut buffer as *mut *const _ as _,
                    0,
                    ptr::null_mut(),
                );

                let msg = String::from_utf8_lossy(slice::from_raw_parts(buffer, len as _));
                let msg = msg.trim_end();

                f.write_str("win32 error: ").and(f.write_str(msg))
            },
            Error::NoContext => f.write_str("tried to use wae outside of a wae context"),
            Error::RecursiveContext => f.write_str("tried to recursively enter a wae context"),
            Error::RecursiveBlockOn => f.write_str("tried to recursively block on a future"),
            Error::Unexpected(msg) => f
                .write_str("unexpected error, please file a bug report at ")
                .and(f.write_str(env!("CARGO_PKG_REPOSITORY")))
                .and(f.write_str(": "))
                .and(f.write_str(msg)),
        }
    }
}

impl Error {
    pub fn win32() -> Self {
        Self::Win32(unsafe { GetLastError() })
    }
}

impl std::error::Error for Error {}
