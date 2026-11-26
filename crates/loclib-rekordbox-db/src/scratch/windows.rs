//! Who a folder belongs to and who may open it, on Windows: the current user's SID, from its
//! process token.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT, SetNamedSecurityInfoW};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACL, ACL_REVISION, AddAccessAllowedAceEx, CONTAINER_INHERIT_ACE, DACL_SECURITY_INFORMATION,
    EqualSid, GetLengthSid, GetTokenInformation, InitializeAcl, OBJECT_INHERIT_ACE, OWNER_SECURITY_INFORMATION,
    PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{FILE_ALL_ACCESS, FILE_FLAG_OPEN_REPARSE_POINT};
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

/// Lets only the current user open the folder at `path` and what's made in it: a protected
/// access list (nothing inherited from the folder it's in) with that user alone.
pub(super) fn private(path: &Path) -> std::io::Result<()> {
    let user = user()?;
    let sid = sid(&user);
    // SAFETY: `sid` is valid while `user` lives.
    let sid_len = unsafe { GetLengthSid(sid) } as usize;
    // An ACL header and one ACE, whose SID starts at its `SidStart` field.
    let size = size_of::<ACL>() + size_of::<ACCESS_ALLOWED_ACE>() - size_of::<u32>() + sid_len;
    let mut buffer = vec![0u64; size.div_ceil(8)];
    let acl = buffer.as_mut_ptr().cast::<ACL>();
    // SAFETY: `acl` holds `size` bytes, aligned for ACL.
    let made = unsafe {
        InitializeAcl(acl, size as u32, ACL_REVISION) != 0
            && AddAccessAllowedAceEx(
                acl,
                ACL_REVISION,
                OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE,
                FILE_ALL_ACCESS,
                sid,
            ) != 0
    };
    if !made {
        return Err(std::io::Error::last_os_error());
    }
    let name = wide(path);
    let info = DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION;
    // SAFETY: `name` is NUL-terminated and `acl` a valid ACL for the call.
    let set = unsafe {
        SetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            info,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            acl,
            std::ptr::null(),
        )
    };
    if set != 0 {
        return Err(std::io::Error::from_raw_os_error(set as i32));
    }
    // Anyone the inherited list let in had the moment between the folder's making and now: what
    // they put in it would be read as ours.
    if std::fs::read_dir(path)?.next().is_some() {
        return Err(std::io::Error::other("something was put in the new folder before it was made private"));
    }
    Ok(())
}

/// Whether the folder at `path`'s access list is protected, and the SIDs it lets in (tests).
#[cfg(test)]
pub(super) fn access(path: &Path) -> (bool, usize, bool) {
    use windows_sys::Win32::Security::{GetAce, GetSecurityDescriptorControl, SE_DACL_PROTECTED};
    let user = user().unwrap();
    let (mut dacl, mut descriptor): (*mut ACL, PSECURITY_DESCRIPTOR) = (std::ptr::null_mut(), std::ptr::null_mut());
    let name = wide(path);
    // SAFETY: as in `owned`; `dacl` points into `descriptor`, freed below.
    let read = unsafe {
        GetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut dacl,
            std::ptr::null_mut(),
            &mut descriptor,
        )
    };
    assert_eq!(read, 0);
    let (mut control, mut revision) = (0u16, 0u32);
    // SAFETY: `descriptor` is valid until freed.
    assert_ne!(unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) }, 0);
    // SAFETY: `dacl` is valid while `descriptor` is.
    let count = usize::from(unsafe { (*dacl).AceCount });
    let mut ace: *mut core::ffi::c_void = std::ptr::null_mut();
    // SAFETY: the first ACE of a list that has one; its SID starts at `SidStart`.
    let first_is_user = count > 0
        && unsafe { GetAce(dacl, 0, &mut ace) } != 0
        && unsafe { EqualSid((&raw mut (*ace.cast::<ACCESS_ALLOWED_ACE>()).SidStart).cast(), sid(&user)) } != 0;
    // SAFETY: allocated by GetNamedSecurityInfoW.
    unsafe { LocalFree(descriptor) };
    (control & SE_DACL_PROTECTED != 0, count, first_is_user)
}
