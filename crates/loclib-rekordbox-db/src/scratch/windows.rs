//! Who a folder belongs to and who may open it, on Windows: the current user's SID, from its
//! process token.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
use windows_sys::Win32::Security::{
    EqualSid, GetTokenInformation, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, TOKEN_QUERY, TOKEN_USER,
    TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// Opens a link itself rather than what it points to.
pub(super) const OPEN_REPARSE_POINT: u32 = FILE_FLAG_OPEN_REPARSE_POINT;

/// The current user's `TOKEN_USER`, whose SID points into the buffer itself (`u64`s, so it's
/// aligned for the pointer at its start).
fn user() -> std::io::Result<Vec<u64>> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: the pseudo-handle GetCurrentProcess returns needs no closing; `token` is written on
    // success and closed below.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut size = 0u32;
    // SAFETY: asking for the size only, with no buffer.
    unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut size) };
    let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
    // SAFETY: `buffer` holds `size` bytes.
    let read = unsafe { GetTokenInformation(token, TokenUser, buffer.as_mut_ptr().cast(), size, &mut size) };
    let error = std::io::Error::last_os_error();
    // SAFETY: `token` is open, and closed once.
    unsafe { CloseHandle(token) };
    if read == 0 {
        return Err(error);
    }
    Ok(buffer)
}

/// The SID in a [`user`] buffer.
fn sid(user: &[u64]) -> PSID {
    // SAFETY: GetTokenInformation filled the buffer with a TOKEN_USER.
    unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid }
}

/// `path` as a NUL-terminated wide string.
fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain([0]).collect()
}

/// Whether the current user owns the folder at `path`.
pub(super) fn owned(path: &Path) -> bool {
    let Ok(user) = user() else { return false };
    let (mut owner, mut descriptor): (PSID, PSECURITY_DESCRIPTOR) = (std::ptr::null_mut(), std::ptr::null_mut());
    let name = wide(path);
    // SAFETY: `name` is NUL-terminated; `owner` points into `descriptor`, freed below.
    let read = unsafe {
        GetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut descriptor,
        )
    };
    if read != 0 {
        return false;
    }
    // SAFETY: both SIDs are valid: `owner` while `descriptor` lives, the user's while `user` does.
    let same = unsafe { EqualSid(owner, sid(&user)) } != 0;
    // SAFETY: GetNamedSecurityInfoW allocated `descriptor` with LocalAlloc.
    unsafe { LocalFree(descriptor) };
    same
}
