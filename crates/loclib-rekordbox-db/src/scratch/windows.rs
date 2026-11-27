//! Who a folder belongs to and who may open it, on Windows: the current user's SID, from its
//! process token.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT, SetNamedSecurityInfoW};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACL, ACL_REVISION, AddAccessAllowedAceEx, CONTAINER_INHERIT_ACE, DACL_SECURITY_INFORMATION,
    EqualSid, GetLengthSid, GetTokenInformation, InitializeAcl, OBJECT_INHERIT_ACE, OBJECT_SECURITY_INFORMATION,
    OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, TOKEN_QUERY,
    TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{FILE_ALL_ACCESS, FILE_FLAG_OPEN_REPARSE_POINT};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// Opens a link itself rather than what it points to.
pub(super) const OPEN_REPARSE_POINT: u32 = FILE_FLAG_OPEN_REPARSE_POINT;

/// The current user's `TOKEN_USER`, whose SID points into the buffer itself (`u64`s, so it's
/// aligned for the pointer at its start). Read the SID with [`sid`] while the buffer lives.
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
    if (size as usize) < size_of::<TOKEN_USER>() || sid(&buffer).is_null() {
        return Err(std::io::Error::other("the process token names no user"));
    }
    Ok(buffer)
}

/// The SID in a [`user`] buffer, valid while the buffer is.
fn sid(user: &[u64]) -> PSID {
    // SAFETY: `user` came from `user`, which checked it holds a whole TOKEN_USER.
    unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid }
}

/// `path` as a NUL-terminated wide string.
fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain([0]).collect()
}

/// A security descriptor GetNamedSecurityInfoW allocated, freed when dropped, whatever path the
/// caller leaves by. The owner and access list it hands back point into it.
struct Descriptor(PSECURITY_DESCRIPTOR);

impl Drop for Descriptor {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: GetNamedSecurityInfoW allocated it with LocalAlloc, and it's freed once.
            unsafe { LocalFree(self.0) };
        }
    }
}

/// The folder at `path`'s security descriptor (`info` of it), with its owner and access list,
/// each null when not asked for or when it has none.
fn security(path: &Path, info: OBJECT_SECURITY_INFORMATION) -> std::io::Result<(Descriptor, PSID, *mut ACL)> {
    let (mut owner, mut dacl, mut descriptor): (PSID, *mut ACL, PSECURITY_DESCRIPTOR) =
        (std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
    let name = wide(path);
    // SAFETY: `name` is NUL-terminated, and each out pointer is a local the call writes.
    let read = unsafe {
        GetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            info,
            &mut owner,
            std::ptr::null_mut(),
            &mut dacl,
            std::ptr::null_mut(),
            &mut descriptor,
        )
    };
    // Owned before anything can return: freed on every path.
    let descriptor = Descriptor(descriptor);
    if read != 0 {
        return Err(std::io::Error::from_raw_os_error(read as i32));
    }
    Ok((descriptor, owner, dacl))
}

/// Whether the current user owns the folder at `path`. A folder without an owner isn't theirs.
pub(super) fn owned(path: &Path) -> bool {
    let Ok(user) = user() else { return false };
    let Ok((_descriptor, owner, _)) = security(path, OWNER_SECURITY_INFORMATION) else { return false };
    if owner.is_null() {
        return false;
    }
    // SAFETY: `owner` is non-null and valid while `_descriptor` lives, the user's SID while
    // `user` does; both live past the call.
    unsafe { EqualSid(owner, sid(&user)) != 0 }
}

/// Lets only the current user open the folder at `path` and what's made in it: a protected
/// access list (nothing inherited from the folder it's in) with that user alone.
pub(super) fn private(path: &Path) -> std::io::Result<()> {
    let user = user()?;
    let sid = sid(&user);
    // SAFETY: `sid` is non-null (checked by `user`) and valid while `user` lives.
    let sid_len = unsafe { GetLengthSid(sid) } as usize;
    // An ACL header and one ACE, whose SID starts at its `SidStart` field.
    let size = size_of::<ACL>() + size_of::<ACCESS_ALLOWED_ACE>() - size_of::<u32>() + sid_len;
    let mut buffer = vec![0u64; size.div_ceil(8)];
    let acl = buffer.as_mut_ptr().cast::<ACL>();
    // SAFETY: `acl` holds `size` bytes, aligned for ACL, and `sid` is valid as above.
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
    // SAFETY: `name` is NUL-terminated and `acl` a valid ACL in `buffer`, which outlives the call.
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

/// The folder at `path`'s access list: whether it's protected (nothing inherited), how many
/// entries it has, and whether the first lets in the current user (tests). A folder without an
/// access list (NULL), which lets everyone in, panics.
#[cfg(test)]
pub(super) fn access(path: &Path) -> (bool, usize, bool) {
    use windows_sys::Win32::Security::{GetAce, GetSecurityDescriptorControl, SE_DACL_PROTECTED};
    let user = user().unwrap();
    let (descriptor, _, dacl) = security(path, DACL_SECURITY_INFORMATION).unwrap();
    let (mut control, mut revision) = (0u16, 0u32);
    // SAFETY: `descriptor` is valid until dropped at the end.
    assert_ne!(unsafe { GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) }, 0);
    // SAFETY: when non-null, `dacl` points into `descriptor`, which lives to the end.
    let list = unsafe { dacl.as_ref() }.expect("a NULL DACL: everyone has access");
    let count = usize::from(list.AceCount);
    let mut first_is_user = false;
    if count > 0 {
        let mut ace: *mut core::ffi::c_void = std::ptr::null_mut();
        // SAFETY: `dacl` is a valid list with an entry 0; `ace` is written on success.
        assert_ne!(unsafe { GetAce(dacl, 0, &mut ace) }, 0, "GetAce failed");
        // SAFETY: when non-null, `ace` points at the list's first entry, an ACCESS_ALLOWED_ACE
        // here, whose SID starts at `SidStart`.
        let entry = unsafe { ace.cast::<ACCESS_ALLOWED_ACE>().as_ref() }.expect("GetAce gave no entry");
        let entry_sid: PSID = (&raw const entry.SidStart).cast_mut().cast();
        // SAFETY: both SIDs are valid: the entry's while `descriptor` lives, the user's while
        // `user` does.
        first_is_user = unsafe { EqualSid(entry_sid, sid(&user)) } != 0;
    }
    (control & SE_DACL_PROTECTED != 0, count, first_is_user)
}

/// Gives the folder at `path` no access list at all (NULL), which lets everyone in (tests).
#[cfg(test)]
pub(super) fn open_to_everyone(path: &Path) {
    let name = wide(path);
    // SAFETY: `name` is NUL-terminated; a null list is what the call is asked to set.
    let set = unsafe {
        SetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    assert_eq!(set, 0);
}
