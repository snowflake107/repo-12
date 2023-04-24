use std::ffi::OsStr;
use std::fs::{File, OpenOptions};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, RawHandle};
use std::path::Path;
use std::{io, iter};

#[allow(non_camel_case_types)]
mod bindings {
    pub const INVALID_HANDLE_VALUE: isize = -1;
    pub type BOOL = i32;
    pub type MOVE_FILE_FLAGS = u32;
    pub const MOVEFILE_REPLACE_EXISTING: MOVE_FILE_FLAGS = 1;
    pub type HANDLE = isize;

    pub type FILE_SHARE_MODE = u32;
    pub const FILE_SHARE_READ: FILE_SHARE_MODE = 1;
    pub const FILE_SHARE_WRITE: FILE_SHARE_MODE = 2;
    pub const FILE_SHARE_DELETE: FILE_SHARE_MODE = 4;

    pub type FILE_FLAGS_AND_ATTRIBUTES = u32;
    pub const FILE_ATTRIBUTE_NORMAL: FILE_FLAGS_AND_ATTRIBUTES = 128;
    pub const FILE_ATTRIBUTE_TEMPORARY: FILE_FLAGS_AND_ATTRIBUTES = 256;
    pub const FILE_FLAG_DELETE_ON_CLOSE: FILE_FLAGS_AND_ATTRIBUTES = 67108864;

    pub const FILE_GENERIC_READ: u32 = 1179785;
    pub const FILE_GENERIC_WRITE: u32 = 1179926;

    #[link(name = "kernel32", kind = "raw-dylib")]
    extern "system" {
        pub fn MoveFileExW(
            lpExistingFileName: *const u16,
            lpNewFileName: *const u16,
            dwFlags: MOVE_FILE_FLAGS,
        ) -> BOOL;
        pub fn ReOpenFile(
            hOriginalFile: HANDLE,
            dwDesiredAccess: u32,
            dwShareMode: FILE_SHARE_MODE,
            dwFlagsAndAttributes: FILE_FLAGS_AND_ATTRIBUTES,
        ) -> HANDLE;
        pub fn SetFileAttributesW(
            lpFileName: *const u16,
            dwFileAttributes: FILE_FLAGS_AND_ATTRIBUTES,
        ) -> BOOL;
    }
}

use bindings::*;

use crate::util;

fn to_utf16(s: &Path) -> Vec<u16> {
    s.as_os_str().encode_wide().chain(iter::once(0)).collect()
}

pub fn create_named(path: &Path, open_options: &mut OpenOptions) -> io::Result<File> {
    open_options
        .create_new(true)
        .read(true)
        .write(true)
        .custom_flags(FILE_ATTRIBUTE_TEMPORARY)
        .open(path)
}

pub fn create(dir: &Path) -> io::Result<File> {
    util::create_helper(
        dir,
        OsStr::new(".tmp"),
        OsStr::new(""),
        crate::NUM_RAND_CHARS,
        |path| {
            OpenOptions::new()
                .create_new(true)
                .read(true)
                .write(true)
                .share_mode(0)
                .custom_flags(FILE_ATTRIBUTE_TEMPORARY | FILE_FLAG_DELETE_ON_CLOSE)
                .open(path)
        },
    )
}

pub fn reopen(file: &File, _path: &Path) -> io::Result<File> {
    let handle = file.as_raw_handle();
    unsafe {
        let handle = ReOpenFile(
            handle as HANDLE,
            FILE_GENERIC_READ | FILE_GENERIC_WRITE,
            FILE_SHARE_DELETE | FILE_SHARE_READ | FILE_SHARE_WRITE,
            0,
        );
        if handle == INVALID_HANDLE_VALUE {
            Err(io::Error::last_os_error())
        } else {
            Ok(FromRawHandle::from_raw_handle(handle as RawHandle))
        }
    }
}

pub fn keep(path: &Path) -> io::Result<()> {
    unsafe {
        let path_w = to_utf16(path);
        if SetFileAttributesW(path_w.as_ptr(), FILE_ATTRIBUTE_NORMAL) == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

pub fn persist(old_path: &Path, new_path: &Path, overwrite: bool) -> io::Result<()> {
    // TODO: We should probably do this in one-shot using SetFileInformationByHandle but the API is
    // really painful.

    unsafe {
        let old_path_w = to_utf16(old_path);
        let new_path_w = to_utf16(new_path);

        // Don't succeed if this fails. We don't want to claim to have successfully persisted a file
        // still marked as temporary because this file won't have the same consistency guarantees.
        if SetFileAttributesW(old_path_w.as_ptr(), FILE_ATTRIBUTE_NORMAL) == 0 {
            return Err(io::Error::last_os_error());
        }

        let mut flags = 0;

        if overwrite {
            flags |= MOVEFILE_REPLACE_EXISTING;
        }

        if MoveFileExW(old_path_w.as_ptr(), new_path_w.as_ptr(), flags) == 0 {
            let e = io::Error::last_os_error();
            // If this fails, the temporary file is now un-hidden and no longer marked temporary
            // (slightly less efficient) but it will still work.
            let _ = SetFileAttributesW(old_path_w.as_ptr(), FILE_ATTRIBUTE_TEMPORARY);
            Err(e)
        } else {
            Ok(())
        }
    }
}
