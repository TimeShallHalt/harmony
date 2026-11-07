//! Windows handle-relative storage. Directory handles deny DELETE, allow WRITE:
//! Windows rename itself needs directory-write sharing. All descendant opens use
//! NtCreateFile RootDirectory + OBJ_DONT_REPARSE, never a reused absolute path.
use super::*;
use std::fs::File;
use std::os::windows::{
    ffi::OsStrExt,
    io::{AsRawHandle, FromRawHandle},
};
type Handle = *mut std::ffi::c_void;
#[repr(C)]
struct Unicode {
    len: u16,
    max: u16,
    buffer: *const u16,
}
#[repr(C)]
struct Object {
    len: u32,
    root: Handle,
    name: *const Unicode,
    flags: u32,
    security: Handle,
    qos: Handle,
}
#[repr(C)]
struct Status {
    status: usize,
    information: usize,
}
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtCreateFile(
        out: *mut Handle,
        access: u32,
        object: *const Object,
        status: *mut Status,
        allocation: *const i64,
        attrs: u32,
        share: u32,
        disposition: u32,
        options: u32,
        ea: Handle,
        ea_len: u32,
    ) -> i32;
    fn NtSetInformationFile(
        file: Handle,
        status: *mut Status,
        info: *const std::ffi::c_void,
        len: u32,
        class: u32,
    ) -> i32;
}
pub fn open(
    parent: &File,
    name: &std::ffi::OsStr,
    directory: bool,
    create: bool,
    delete: bool,
) -> Result<File> {
    let wide: Vec<u16> = name.encode_wide().collect();
    if wide.len() > 255
        || wide
            .iter()
            .any(|c| [0, b'/' as u16, b'\\' as u16, b':' as u16].contains(c))
    {
        return Err(Error::Invalid("Windows relative name"));
    }
    let u = Unicode {
        len: (wide.len() * 2) as u16,
        max: (wide.len() * 2) as u16,
        buffer: wide.as_ptr(),
    };
    let object = Object {
        len: std::mem::size_of::<Object>() as u32,
        root: parent.as_raw_handle(),
        name: &u,
        flags: 0x40 | 0x1000,
        security: std::ptr::null_mut(),
        qos: std::ptr::null_mut(),
    };
    let mut h = std::ptr::null_mut();
    let mut status = Status {
        status: 0,
        information: 0,
    };
    let access = 0x8010_0000
        | if create && !directory { 0x4000_0000 } else { 0 }
        | if delete { 0x0001_0000 } else { 0 };
    let options = 0x0020_0000 | 0x20 | if directory { 1 } else { 0x40 };
    // No output pointer escapes, all buffers live across the synchronous syscall.
    let result = unsafe {
        NtCreateFile(
            &mut h,
            access,
            &object,
            &mut status,
            std::ptr::null(),
            0x80,
            if directory { 3 } else { 1 | 4 },
            if create { 2 } else { 1 },
            options,
            std::ptr::null_mut(),
            0,
        )
    };
    if result < 0 {
        return Err(if result as u32 == 0xc0000034 {
            Error::NotFound
        } else {
            Error::Io("open handle-relative update entry (links refused)")
        });
    }
    let file = unsafe { File::from_raw_handle(h) };
    use std::os::windows::fs::MetadataExt;
    let meta = file
        .metadata()
        .map_err(|_| Error::Io("Windows handle metadata"))?;
    if meta.file_attributes() & 0x400 != 0
        || (directory && !meta.is_dir())
        || (!directory && !meta.is_file())
    {
        return Err(Error::Invalid("Windows reparse/nonregular entry"));
    }
    if !directory {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetFileInformationByHandle(file: Handle, info: *mut u32) -> i32;
        }
        let mut info = [0u32; 13];
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) } == 0
            || info[10] != 1
        {
            return Err(Error::Invalid("multiply linked update file"));
        }
    }
    Ok(file)
}
/// Require a user/admin-owned DACL with no write grant to untrusted SIDs.
/// Read-only access for other users is harmless: update files contain no secrets.
pub fn private(file: &File) -> Result<()> {
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn GetSecurityInfo(
            handle: Handle,
            kind: u32,
            flags: u32,
            owner: *mut Handle,
            group: *mut Handle,
            dacl: *mut Handle,
            sacl: *mut Handle,
            descriptor: *mut Handle,
        ) -> u32;
        fn OpenProcessToken(process: Handle, access: u32, token: *mut Handle) -> i32;
        fn GetTokenInformation(
            token: Handle,
            class: u32,
            buffer: Handle,
            length: u32,
            returned: *mut u32,
        ) -> i32;
        fn EqualSid(a: Handle, b: Handle) -> i32;
        fn GetAce(acl: Handle, index: u32, ace: *mut Handle) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> Handle;
        fn CloseHandle(handle: Handle) -> i32;
        fn LocalFree(handle: Handle) -> Handle;
    }
    struct Descriptor(Handle);
    impl Drop for Descriptor {
        fn drop(&mut self) {
            unsafe {
                LocalFree(self.0);
            }
        }
    }
    struct Token(Handle);
    impl Drop for Token {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    let mut owner = std::ptr::null_mut();
    let mut acl = std::ptr::null_mut();
    let mut descriptor = std::ptr::null_mut();
    if unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            1,
            1 | 4,
            &mut owner,
            std::ptr::null_mut(),
            &mut acl,
            std::ptr::null_mut(),
            &mut descriptor,
        )
    } != 0
    {
        return Err(Error::Io("inspect staging ACL"));
    }
    let _descriptor = Descriptor(descriptor);
    if owner.is_null() || acl.is_null() {
        return Err(Error::Invalid("staging requires an explicit owned ACL"));
    }
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), 8, &mut token) } == 0 {
        return Err(Error::Io("inspect staging owner"));
    }
    let token = Token(token);
    let mut size = 0u32;
    unsafe {
        GetTokenInformation(token.0, 1, std::ptr::null_mut(), 0, &mut size);
    }
    if size == 0 || size > 65536 {
        return Err(Error::Io("inspect current user SID"));
    }
    let mut buffer = vec![0u8; size as usize];
    if unsafe { GetTokenInformation(token.0, 1, buffer.as_mut_ptr().cast(), size, &mut size) } == 0
    {
        return Err(Error::Io("inspect current user SID"));
    }
    let user = unsafe { std::ptr::read_unaligned(buffer.as_ptr().cast::<Handle>()) };
    fn sid(sub: &[u32]) -> Vec<u8> {
        let mut s = vec![1, sub.len() as u8, 0, 0, 0, 0, 0, 5];
        for n in sub {
            s.extend_from_slice(&n.to_le_bytes());
        }
        s
    }
    let mut system = sid(&[18]);
    let mut administrators = sid(&[32, 544]);
    let trusted = |who: Handle| unsafe {
        EqualSid(who, user) != 0
            || EqualSid(who, system.as_mut_ptr().cast()) != 0
            || EqualSid(who, administrators.as_mut_ptr().cast()) != 0
    };
    let mut trusted = trusted;
    if !trusted(owner) {
        return Err(Error::Invalid(
            "staging not owned by this user or system administrators",
        ));
    }
    // ACLs are kernel-validated security descriptors. GetAce supplies each
    // bounded ACE; reject complex granting ACEs rather than interpret GUIDs.
    let count = unsafe { std::ptr::read_unaligned((acl as *const u8).add(4).cast::<u16>()) };
    for i in 0..u32::from(count) {
        let mut ace = std::ptr::null_mut();
        if unsafe { GetAce(acl, i, &mut ace) } == 0 || ace.is_null() {
            return Err(Error::Io("inspect staging permissions"));
        }
        let p = ace as *const u8;
        let kind = unsafe { *p };
        let flags = unsafe { *p.add(1) };
        if flags & 8 != 0 || matches!(kind, 1 | 6 | 10 | 12) {
            continue;
        } // inherit-only / deny ACEs
        let size = unsafe { std::ptr::read_unaligned(p.add(2).cast::<u16>()) };
        if size < 8 {
            return Err(Error::Invalid("staging ACL entry"));
        }
        let mask = unsafe { std::ptr::read_unaligned(p.add(4).cast::<u32>()) };
        let writes = 0x1000_0000
            | 0x4000_0000
            | 0x0001_0000
            | 0x0004_0000
            | 0x0008_0000
            | 2
            | 4
            | 0x10
            | 0x40
            | 0x100;
        if mask & writes == 0 {
            continue;
        }
        if kind != 0 || size < 12 || !trusted(unsafe { p.add(8) } as Handle) {
            return Err(Error::Invalid("staging writable by an untrusted principal"));
        }
    }
    Ok(())
}
pub fn rename(file: &File, dest: &File, name: &str, replace: bool) -> Result<()> {
    #[repr(C)]
    struct Rename {
        replace: u8,
        pad: [u8; 7],
        root: Handle,
        len: u32,
        name: [u16; 192],
    }
    let mut r = Rename {
        replace: u8::from(replace),
        pad: [0; 7],
        root: dest.as_raw_handle(),
        len: 0,
        name: [0; 192],
    };
    for (i, c) in name.encode_utf16().enumerate() {
        r.name[i] = c;
        r.len += 2;
    }
    let mut status = Status {
        status: 0,
        information: 0,
    };
    if unsafe {
        NtSetInformationFile(
            file.as_raw_handle(),
            &mut status,
            (&r as *const Rename).cast(),
            20 + r.len,
            10,
        )
    } < 0
    {
        return Err(Error::Io("rename handle-relative update file"));
    }
    Ok(())
}
pub fn remove(file: &File) -> Result<()> {
    let delete: u8 = 1;
    let mut status = Status {
        status: 0,
        information: 0,
    };
    if unsafe {
        NtSetInformationFile(
            file.as_raw_handle(),
            &mut status,
            (&delete as *const u8).cast(),
            1,
            13,
        )
    } < 0
    {
        return Err(Error::Io("remove handle-relative update file"));
    }
    Ok(())
}
