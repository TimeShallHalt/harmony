//! Same-user, local-only overlapped Windows pipes. No TCP/WebSocket server.
use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use windows_sys::Win32::{
    Security::*,
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

pub(super) struct Socket {
    listener: NamedPipeServer,
    pub(super) path: String,
}

/// Keep token/ACL backing allocations stable until CreateNamedPipe copies the
/// descriptor. The descriptor is never NULL and handles are not inheritable.
struct Security {
    descriptor: Box<SECURITY_DESCRIPTOR>,
    _acl: Vec<u64>,
    _token: Vec<u64>,
}
impl Security {
    fn new() -> io::Result<Self> {
        // SAFETY: all out-pointers reference live, suitably aligned allocations;
        // the token is query-only and closed on every path by OwnedHandle.
        unsafe {
            let mut handle = std::ptr::null_mut();
            ok(OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_QUERY,
                &mut handle,
            ))?;
            let token_handle = OwnedHandle::from_raw_handle(handle);
            let mut bytes = 0;
            GetTokenInformation(
                token_handle.as_raw_handle(),
                TokenUser,
                std::ptr::null_mut(),
                0,
                &mut bytes,
            );
            if bytes == 0 || bytes > 4096 {
                return Err(io::Error::last_os_error());
            }
            let mut token = vec![0_u64; (bytes as usize).div_ceil(8)];
            ok(GetTokenInformation(
                token_handle.as_raw_handle(),
                TokenUser,
                token.as_mut_ptr().cast(),
                bytes,
                &mut bytes,
            ))?;
            let user = (*(token.as_ptr().cast::<TOKEN_USER>())).User.Sid;
            let mut network = [0_u64; 9];
            let mut network_len = std::mem::size_of_val(&network) as u32;
            ok(CreateWellKnownSid(
                WinNetworkSid,
                std::ptr::null_mut(),
                network.as_mut_ptr().cast(),
                &mut network_len,
            ))?;
            let mut acl = vec![0_u64; 128];
            let pacl = acl.as_mut_ptr().cast::<ACL>();
            ok(InitializeAcl(pacl, (acl.len() * 8) as u32, ACL_REVISION))?;
            // Deny network logons even if they have the same account SID.
            ok(AddAccessDeniedAce(
                pacl,
                ACL_REVISION,
                0x10000000,
                network.as_mut_ptr().cast(),
            ))?;
            ok(AddAccessAllowedAce(pacl, ACL_REVISION, 0x10000000, user))?;
            let mut descriptor = Box::<SECURITY_DESCRIPTOR>::default();
            let psd = (&mut *descriptor as *mut SECURITY_DESCRIPTOR).cast();
            ok(InitializeSecurityDescriptor(psd, 1))?;
            ok(SetSecurityDescriptorDacl(psd, 1, pacl, 0))?;
            ok(SetSecurityDescriptorControl(
                psd,
                SE_DACL_PROTECTED,
                SE_DACL_PROTECTED,
            ))?;
            Ok(Self {
                descriptor,
                _acl: acl,
                _token: token,
            })
        }
    }
    fn create(&mut self, path: &str, first: bool) -> io::Result<NamedPipeServer> {
        let mut attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: (&mut *self.descriptor as *mut SECURITY_DESCRIPTOR).cast(),
            bInheritHandle: 0,
        };
        // SAFETY: attributes and the complete descriptor/ACL/SID remain live
        // until the synchronous create call returns. Tokio owns the new handle.
        unsafe {
            ServerOptions::new()
                .first_pipe_instance(first)
                .reject_remote_clients(true)
                .max_instances(17)
                .in_buffer_size(65536)
                .out_buffer_size(65536)
                .create_with_security_attributes_raw(
                    path,
                    (&mut attributes as *mut SECURITY_ATTRIBUTES).cast(),
                )
        }
    }
}
fn ok(value: i32) -> io::Result<()> {
    if value == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
impl Socket {
    pub(super) fn bind_named(path: &str) -> io::Result<Self> {
        // Never accept a UNC host/path or a remote redirector supplied by an envvar.
        let tail = path
            .strip_prefix(r"\\.\pipe\")
            .filter(|s| !s.is_empty() && s.len() <= 128 && !s.contains(['\\', '/', '\0']));
        if tail.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "RPC requires a local named pipe",
            ));
        }
        let listener = Security::new()?.create(path, true)?;
        Ok(Self {
            listener,
            path: path.into(),
        })
    }
    pub(super) async fn accept(&mut self) -> io::Result<NamedPipeServer> {
        self.listener.connect().await?;
        // Keep a reserved server instance across each handoff; do not let a
        // disconnect release the pipe name and permit another owner to take it.
        match Security::new()?.create(&self.path, false) {
            Ok(next) => Ok(std::mem::replace(&mut self.listener, next)),
            Err(error) => {
                let _ = self.listener.disconnect();
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::windows::named_pipe::ClientOptions;

    fn private_name() -> String {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        format!(
            r"\\.\pipe\harmony-presence-fixture-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )
    }
    #[tokio::test]
    async fn kernel_pipe_acl_allows_only_current_user_and_explicitly_denies_network() {
        let name = private_name();
        let socket = Socket::bind_named(&name).unwrap();
        let identity = Security::new().unwrap();
        // Read the actual kernel object's descriptor, not our construction plan.
        unsafe {
            let mut bytes = 0;
            GetKernelObjectSecurity(
                socket.listener.as_raw_handle(),
                DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                0,
                &mut bytes,
            );
            assert!(bytes > 0 && bytes < 4096);
            let mut descriptor = vec![0_u64; (bytes as usize).div_ceil(8)];
            ok(GetKernelObjectSecurity(
                socket.listener.as_raw_handle(),
                DACL_SECURITY_INFORMATION,
                descriptor.as_mut_ptr().cast(),
                bytes,
                &mut bytes,
            ))
            .unwrap();
            let mut present = 0;
            let mut defaulted = 0;
            let mut acl = std::ptr::null_mut();
            ok(GetSecurityDescriptorDacl(
                descriptor.as_mut_ptr().cast(),
                &mut present,
                &mut acl,
                &mut defaulted,
            ))
            .unwrap();
            assert_eq!(present, 1);
            assert!(!acl.is_null());
            assert_eq!((*acl).AceCount, 2);
            let mut control = 0;
            let mut revision = 0;
            ok(GetSecurityDescriptorControl(
                descriptor.as_mut_ptr().cast(),
                &mut control,
                &mut revision,
            ))
            .unwrap();
            assert_ne!(control & SE_DACL_PROTECTED, 0);
            let mut deny = std::ptr::null_mut();
            let mut allow = std::ptr::null_mut();
            ok(GetAce(acl, 0, &mut deny)).unwrap();
            ok(GetAce(acl, 1, &mut allow)).unwrap();
            let deny = &mut *(deny.cast::<ACCESS_DENIED_ACE>());
            let allow = &mut *(allow.cast::<ACCESS_ALLOWED_ACE>());
            assert_eq!(deny.Header.AceType, 1);
            assert_eq!(allow.Header.AceType, 0);
            assert_ne!(
                IsWellKnownSid(std::ptr::addr_of_mut!(deny.SidStart).cast(), WinNetworkSid),
                0
            );
            let current = (*(identity._token.as_ptr().cast::<TOKEN_USER>())).User.Sid;
            assert_ne!(
                EqualSid(std::ptr::addr_of_mut!(allow.SidStart).cast(), current),
                0
            );
        }
    }

    #[tokio::test]
    async fn remote_names_are_rejected_without_contacting_a_redirector() {
        for path in [
            r"\\remote\pipe\fixture",
            r"\\.\pipe\nested\fixture",
            "",
            r"\\.\pipe\",
        ] {
            assert!(Socket::bind_named(path).is_err());
        }
    }

    #[tokio::test]
    async fn private_pipe_roundtrip_exclusive_owner_and_drop_cleanup() {
        let name = private_name();
        let mut socket = Socket::bind_named(&name).unwrap();
        assert!(
            Socket::bind_named(&name).is_err(),
            "do not reuse another server's pipe"
        );
        let mut client = ClientOptions::new().open(&name).unwrap();
        let mut server = socket.accept().await.unwrap();
        client.write_all(b"owned").await.unwrap();
        let mut bytes = [0; 5];
        server.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"owned");
        drop(client);
        drop(server);
        drop(socket);
        assert!(
            ClientOptions::new().open(&name).is_err(),
            "no listener survives owner drop"
        );
        // Tokio defers handle deregistration to its I/O driver. A cancelled
        // owner must release the name within a bounded async turn, not require
        // synchronous kernel teardown inside Drop.
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if Socket::bind_named(&name).is_ok() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
    }
}
