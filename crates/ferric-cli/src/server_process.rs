//! Platform-exact process identity and lifecycle control for `ferric server`.
//!
//! Destructive lifecycle operations must never turn a numeric PID from a
//! runfile directly into a kill command.  `LiveProcess` first acquires an OS
//! object which continues to identify the same process even if the numeric PID
//! is later reused: a process `HANDLE` on Windows and a pidfd on Linux.  The
//! handle remains owned until the `LiveProcess` is dropped.

use std::fmt;
use std::path::PathBuf;
use std::process::Child;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Process coordinates persisted in a versioned server runfile.
///
/// `start_token` is deliberately opaque to callers.  It includes the native
/// process-creation coordinate and, on Linux, the boot ID so a retained
/// runfile cannot match a process after a reboot merely because start ticks
/// and a PID happen to repeat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessIdentity {
    pub start_token: String,
    pub executable: PathBuf,
    pub argv: Vec<String>,
}

/// Validate the canonical process-start token for the current supported OS.
///
/// The persisted representation remains an opaque string, but schema-v2
/// authority is accepted only when the string has the exact form emitted by
/// this target's native process adapter. Unsupported targets cannot authorize
/// a schema-v2 process identity.
pub(crate) fn validate_start_token(start_token: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        let value = start_token
            .strip_prefix("windows-filetime:")
            .ok_or_else(|| {
                "expected canonical Windows start token `windows-filetime:<positive-u64>`"
                    .to_string()
            })?;
        parse_canonical_positive_u64(value).ok_or_else(|| {
            "Windows FILETIME must be a canonical positive decimal u64".to_string()
        })?;
        Ok(())
    }

    #[cfg(all(
        target_os = "linux",
        target_endian = "little",
        target_pointer_width = "64",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    {
        let value = start_token.strip_prefix("linux-boot-id:").ok_or_else(|| {
            "expected canonical Linux start token `linux-boot-id:<uuid>;start-ticks:<positive-u64>`"
                .to_string()
        })?;
        let (boot_id, start_ticks) = value.split_once(";start-ticks:").ok_or_else(|| {
            "Linux start token must contain one `;start-ticks:` field".to_string()
        })?;
        if !is_canonical_lowercase_uuid(boot_id) {
            return Err("Linux boot ID must be a canonical lowercase UUID".to_string());
        }
        parse_canonical_positive_u64(start_ticks).ok_or_else(|| {
            "Linux process start ticks must be a canonical positive decimal u64".to_string()
        })?;
        Ok(())
    }

    #[cfg(not(any(
        windows,
        all(
            target_os = "linux",
            target_endian = "little",
            target_pointer_width = "64",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    )))]
    {
        let _ = start_token;
        Err(format!(
            "schema-v2 process start-token authority is unsupported on {}",
            std::env::consts::OS
        ))
    }
}

#[cfg(test)]
pub(crate) fn canonical_test_start_token(coordinate: u64) -> String {
    assert!(
        coordinate > 0,
        "test start-token coordinate must be positive"
    );

    #[cfg(windows)]
    {
        format!("windows-filetime:{coordinate}")
    }

    #[cfg(all(
        target_os = "linux",
        target_endian = "little",
        target_pointer_width = "64",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    {
        format!("linux-boot-id:00000000-1111-4222-8333-444444444444;start-ticks:{coordinate}")
    }

    #[cfg(not(any(
        windows,
        all(
            target_os = "linux",
            target_endian = "little",
            target_pointer_width = "64",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    )))]
    {
        panic!("no canonical schema-v2 test start token exists on an unsupported target")
    }
}

#[cfg(any(
    windows,
    all(
        target_os = "linux",
        target_endian = "little",
        target_pointer_width = "64",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
))]
fn parse_canonical_positive_u64(value: &str) -> Option<u64> {
    if value.is_empty()
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return None;
    }
    value.parse::<u64>().ok().filter(|parsed| *parsed > 0)
}

#[cfg(all(
    target_os = "linux",
    target_endian = "little",
    target_pointer_width = "64",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn is_canonical_lowercase_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                *byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(byte)
            }
        })
}

/// Ownership of sockets able to accept the expected loopback endpoint.
///
/// Platform adapters conservatively include wildcard binds on the registered
/// port because they can also receive traffic addressed to loopback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListenerState {
    /// The target owns only loopback-bound listeners on the registered port.
    OwnedByTarget,
    /// The target owns a wildcard or potentially dual-stack listener on the
    /// registered port. This is non-exclusive, non-authorizing state even
    /// when the exact retained process owns the socket.
    OwnedByTargetWildcard,
    Absent,
    /// The relevant owner set is not exclusively the target. The vector is
    /// complete and includes the target PID too when ownership is shared.
    OwnedByOther(Vec<u32>),
    Uninspectable(String),
}

impl ListenerState {
    /// Only an exclusive IPv4-loopback owner or a proven-absent listener can
    /// authorize destructive lifecycle work.
    pub fn permits_teardown(&self) -> bool {
        matches!(self, Self::OwnedByTarget | Self::Absent)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessFacts {
    pub identity: ProcessIdentity,
    pub listener: ListenerState,
}

/// Typed acquisition failures let a caller distinguish a stale registration
/// from a platform or permission failure without parsing an error string.
#[derive(Debug, PartialEq, Eq)]
pub enum ProcessError {
    NotFound(u32),
    #[cfg_attr(windows, allow(dead_code))]
    Unsupported(&'static str),
    Operation(String),
}

impl fmt::Display for ProcessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(pid) => write!(formatter, "process {pid} was not found"),
            Self::Unsupported(platform) => write!(
                formatter,
                "exact managed-process control is unsupported on {platform}"
            ),
            Self::Operation(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for ProcessError {}

/// Operations available only after an exact OS process object has been
/// retained. Implementations must never reopen the numeric PID for signal or
/// wait authority.
pub trait RetainedProcess {
    fn pid(&self) -> u32;
    fn inspect(&self, port: u16) -> Result<ProcessFacts, ProcessError>;
    fn terminate(&self) -> Result<bool, ProcessError>;
    fn wait(&self, timeout: Duration) -> Result<bool, ProcessError>;
}

/// Acquisition boundary used by lifecycle orchestration and deterministic
/// generation-reuse tests.
pub trait ProcessRuntime {
    type Process: RetainedProcess;

    fn acquire(&self, pid: u32) -> Result<Self::Process, ProcessError>;
}

/// Facts inspected from the same retained process object that later receives
/// any signal/wait operation.
pub struct RetainedProcessInspection<P> {
    pub process: P,
    pub facts: ProcessFacts,
}

/// Acquire first, then validate all persisted v2 identity coordinates against
/// that retained object. A mismatch returns without signaling it.
pub fn acquire_matching_process<R: ProcessRuntime>(
    runtime: &R,
    pid: u32,
    port: u16,
    expected: &ProcessIdentity,
) -> Result<RetainedProcessInspection<R::Process>, ProcessError> {
    let process = runtime.acquire(pid)?;
    let facts = process.inspect(port)?;
    let mismatch = if facts.identity.start_token != expected.start_token {
        Some("start token")
    } else if facts.identity.executable != expected.executable {
        Some("executable")
    } else if facts.identity.argv != expected.argv {
        Some("argv")
    } else {
        None
    };
    if let Some(coordinate) = mismatch {
        return Err(ProcessError::Operation(format!(
            "retained process {pid} {coordinate} does not match the registered identity"
        )));
    }
    Ok(RetainedProcessInspection { process, facts })
}

/// Inspect loopback-relevant listener ownership independently of process
/// liveness.
///
/// This is intentionally separate from `LiveProcess::inspect`: teardown uses
/// it after the retained process object has exited to prove that the target PID
/// no longer owns the registered listener.
pub fn loopback_listener_state(pid: u32, port: u16) -> ListenerState {
    platform::loopback_listener_state(pid, port)
}

#[cfg(windows)]
mod platform {
    use std::ffi::{OsString, c_void};
    use std::mem::{MaybeUninit, size_of};
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::io::AsRawHandle;
    use std::ptr;

    use super::*;

    type Handle = *mut c_void;

    const PROCESS_TERMINATE: u32 = 0x0001;
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const WAIT_OBJECT_0: u32 = 0;
    const WAIT_TIMEOUT: u32 = 258;
    const WAIT_FAILED: u32 = u32::MAX;
    const PROCESS_COMMAND_LINE_INFORMATION: u32 = 60;
    const DUPLICATE_SAME_ACCESS: u32 = 0x0000_0002;
    const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004_u32 as i32;
    const STATUS_BUFFER_TOO_SMALL: i32 = 0xC000_0023_u32 as i32;

    const AF_INET: u32 = 2;
    const AF_INET6: u32 = 23;
    const TCP_TABLE_OWNER_PID_LISTENER: u32 = 3;
    const MIB_TCP_STATE_LISTEN: u32 = 2;
    const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
    const MAX_NATIVE_QUERY_RESIZES: usize = 4;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct FileTime {
        low_date_time: u32,
        high_date_time: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct UnicodeString {
        length: u16,
        maximum_length: u16,
        buffer: *const u16,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct MibTcpRowOwnerPid {
        state: u32,
        local_address: u32,
        local_port: u32,
        remote_address: u32,
        remote_port: u32,
        owning_pid: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct MibTcp6RowOwnerPid {
        local_address: [u8; 16],
        local_scope_id: u32,
        local_port: u32,
        remote_address: [u8; 16],
        remote_scope_id: u32,
        remote_port: u32,
        state: u32,
        owning_pid: u32,
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct RelevantListenerOwners {
        loopback: Vec<u32>,
        wildcard: Vec<u32>,
        unsupported_ipv6_loopback: Vec<u32>,
    }

    impl RelevantListenerOwners {
        fn extend(&mut self, other: Self) {
            self.loopback.extend(other.loopback);
            self.wildcard.extend(other.wildcard);
            self.unsupported_ipv6_loopback
                .extend(other.unsupported_ipv6_loopback);
            self.normalize();
        }

        fn normalize(&mut self) {
            self.loopback.sort_unstable();
            self.loopback.dedup();
            self.wildcard.sort_unstable();
            self.wildcard.dedup();
            self.unsupported_ipv6_loopback.sort_unstable();
            self.unsupported_ipv6_loopback.dedup();
        }

        fn all(&self) -> Vec<u32> {
            let mut owners = self.loopback.clone();
            owners.extend(self.wildcard.iter().copied());
            owners.sort_unstable();
            owners.dedup();
            owners
        }
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> Handle;
        fn GetCurrentProcess() -> Handle;
        fn DuplicateHandle(
            source_process: Handle,
            source_handle: Handle,
            target_process: Handle,
            target_handle: *mut Handle,
            desired_access: u32,
            inherit_handle: i32,
            options: u32,
        ) -> i32;
        fn CloseHandle(object: Handle) -> i32;
        #[cfg(test)]
        fn GetHandleInformation(object: Handle, flags: *mut u32) -> i32;
        fn GetProcessTimes(
            process: Handle,
            creation_time: *mut FileTime,
            exit_time: *mut FileTime,
            kernel_time: *mut FileTime,
            user_time: *mut FileTime,
        ) -> i32;
        fn QueryFullProcessImageNameW(
            process: Handle,
            flags: u32,
            executable_name: *mut u16,
            size: *mut u32,
        ) -> i32;
        fn TerminateProcess(process: Handle, exit_code: u32) -> i32;
        fn WaitForSingleObject(object: Handle, milliseconds: u32) -> u32;
    }

    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtQueryInformationProcess(
            process: Handle,
            process_information_class: u32,
            process_information: *mut c_void,
            process_information_length: u32,
            return_length: *mut u32,
        ) -> i32;
    }

    #[link(name = "iphlpapi")]
    unsafe extern "system" {
        fn GetExtendedTcpTable(
            tcp_table: *mut c_void,
            size: *mut u32,
            ordered: i32,
            address_family: u32,
            table_class: u32,
            reserved: u32,
        ) -> u32;
    }

    /// A retained Windows process HANDLE.  Teardown always targets `handle`,
    /// never a PID looked up after validation.
    pub struct LiveProcess {
        pid: u32,
        handle: Handle,
    }

    impl LiveProcess {
        /// Duplicate the exact process object already owned by `Child`.
        /// This avoids reopening the spawned process through a numeric PID.
        pub fn acquire_child(child: &Child) -> Result<Self, ProcessError> {
            let pid = child.id();
            let source = child.as_raw_handle() as Handle;
            if pid == 0 || source.is_null() {
                return Err(ProcessError::NotFound(pid));
            }
            let current = unsafe { GetCurrentProcess() };
            let mut handle = ptr::null_mut();
            if unsafe {
                DuplicateHandle(
                    current,
                    source,
                    current,
                    &mut handle,
                    0,
                    0,
                    DUPLICATE_SAME_ACCESS,
                )
            } == 0
            {
                return Err(ProcessError::Operation(format!(
                    "duplicate spawned child process HANDLE for PID {pid}: {}",
                    std::io::Error::last_os_error()
                )));
            }
            Ok(Self { pid, handle })
        }

        pub fn acquire(pid: u32) -> Result<Self, ProcessError> {
            if pid == 0 {
                return Err(ProcessError::NotFound(pid));
            }
            let handle = unsafe {
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | SYNCHRONIZE,
                    0,
                    pid,
                )
            };
            if handle.is_null() {
                let error = std::io::Error::last_os_error();
                // ERROR_INVALID_PARAMETER is the normal result for a PID which
                // does not identify a process.  ERROR_NOT_FOUND is included for
                // compatibility with alternate Windows implementations.
                if matches!(error.raw_os_error(), Some(87 | 1168)) {
                    return Err(ProcessError::NotFound(pid));
                }
                return Err(ProcessError::Operation(format!(
                    "open process {pid} for exact lifecycle control: {error}"
                )));
            }
            Ok(Self { pid, handle })
        }

        pub fn pid(&self) -> u32 {
            self.pid
        }

        pub fn inspect(&self, port: u16) -> Result<ProcessFacts, ProcessError> {
            self.require_active()?;
            let (start_token, executable) = self.native_identity()?;
            let command_line = self.native_command_line()?;
            let argv = split_windows_command_line(&command_line);
            if argv.is_empty() {
                return Err(ProcessError::Operation(format!(
                    "Windows process inspection returned no argv for PID {}",
                    self.pid
                )));
            }

            // The TCP owner table is PID-indexed. Holding the HANDLE makes
            // termination exact; the post-query check prevents combining
            // socket facts from a newly reused PID with the retained, exited
            // object.
            let listener = loopback_listener_state(self.pid, port);
            self.require_active()?;
            Ok(ProcessFacts {
                identity: ProcessIdentity {
                    start_token,
                    executable,
                    argv,
                },
                listener,
            })
        }

        /// Request termination through the retained HANDLE.
        ///
        /// `false` means the exact retained process had already exited; no
        /// other process was signalled.
        pub fn terminate(&self) -> Result<bool, ProcessError> {
            if self.wait(Duration::ZERO)? {
                return Ok(false);
            }
            if unsafe { TerminateProcess(self.handle, 1) } == 0 {
                let error = std::io::Error::last_os_error();
                if self.wait(Duration::ZERO)? {
                    return Ok(false);
                }
                return Err(ProcessError::Operation(format!(
                    "terminate retained process HANDLE for PID {}: {error}",
                    self.pid
                )));
            }
            Ok(true)
        }

        /// Wait at most `timeout` for the retained process object to exit.
        /// Returns `true` when exited and `false` on timeout.
        pub fn wait(&self, timeout: Duration) -> Result<bool, ProcessError> {
            let milliseconds = duration_to_windows_millis(timeout);
            match unsafe { WaitForSingleObject(self.handle, milliseconds) } {
                WAIT_OBJECT_0 => Ok(true),
                WAIT_TIMEOUT => Ok(false),
                WAIT_FAILED => Err(ProcessError::Operation(format!(
                    "wait for retained process HANDLE for PID {}: {}",
                    self.pid,
                    std::io::Error::last_os_error()
                ))),
                result => Err(ProcessError::Operation(format!(
                    "wait for retained process HANDLE for PID {} returned unexpected status {result}",
                    self.pid
                ))),
            }
        }

        fn require_active(&self) -> Result<(), ProcessError> {
            if self.wait(Duration::ZERO)? {
                Err(ProcessError::NotFound(self.pid))
            } else {
                Ok(())
            }
        }

        fn native_identity(&self) -> Result<(String, PathBuf), ProcessError> {
            let mut creation = MaybeUninit::<FileTime>::uninit();
            let mut exit = MaybeUninit::<FileTime>::uninit();
            let mut kernel = MaybeUninit::<FileTime>::uninit();
            let mut user = MaybeUninit::<FileTime>::uninit();
            if unsafe {
                GetProcessTimes(
                    self.handle,
                    creation.as_mut_ptr(),
                    exit.as_mut_ptr(),
                    kernel.as_mut_ptr(),
                    user.as_mut_ptr(),
                )
            } == 0
            {
                return Err(ProcessError::Operation(format!(
                    "read creation time for retained process HANDLE for PID {}: {}",
                    self.pid,
                    std::io::Error::last_os_error()
                )));
            }
            let creation = unsafe { creation.assume_init() };
            let filetime =
                (u64::from(creation.high_date_time) << 32) | u64::from(creation.low_date_time);

            // The extended Windows path limit is 32,767 UTF-16 code units.
            let mut buffer = vec![0_u16; 32_768];
            let mut length = buffer.len() as u32;
            if unsafe {
                QueryFullProcessImageNameW(self.handle, 0, buffer.as_mut_ptr(), &mut length)
            } == 0
            {
                return Err(ProcessError::Operation(format!(
                    "read executable path for retained process HANDLE for PID {}: {}",
                    self.pid,
                    std::io::Error::last_os_error()
                )));
            }
            buffer.truncate(length as usize);
            let executable = PathBuf::from(OsString::from_wide(&buffer));
            if executable.as_os_str().is_empty() {
                return Err(ProcessError::Operation(format!(
                    "Windows process inspection returned an empty executable path for PID {}",
                    self.pid
                )));
            }
            Ok((format_windows_start_token(filetime), executable))
        }

        fn native_command_line(&self) -> Result<String, ProcessError> {
            query_process_command_line(self.handle).map_err(|detail| {
                ProcessError::Operation(format!(
                    "read command line from retained process HANDLE for PID {}: {detail}",
                    self.pid
                ))
            })
        }

        fn close_handle(&mut self) -> Result<(), ProcessError> {
            if self.handle.is_null() {
                return Ok(());
            }

            if unsafe { CloseHandle(self.handle) } == 0 {
                return Err(ProcessError::Operation(format!(
                    "release retained process HANDLE for PID {}: {}",
                    self.pid,
                    std::io::Error::last_os_error()
                )));
            }

            self.handle = ptr::null_mut();
            Ok(())
        }

        #[cfg(test)]
        fn release_for_test(mut self) -> Result<(), ProcessError> {
            self.close_handle()
        }
    }

    impl Drop for LiveProcess {
        fn drop(&mut self) {
            let _ = self.close_handle();
        }
    }

    // The HANDLE is an owned kernel object and Windows permits it to be waited
    // on and queried from other threads.  No method mutates the handle value.
    unsafe impl Send for LiveProcess {}
    unsafe impl Sync for LiveProcess {}

    fn query_process_command_line(handle: Handle) -> Result<String, String> {
        let mut required = 0_u32;
        let status = unsafe {
            NtQueryInformationProcess(
                handle,
                PROCESS_COMMAND_LINE_INFORMATION,
                ptr::null_mut(),
                0,
                &mut required,
            )
        };
        if status < 0
            && !matches!(
                status,
                STATUS_INFO_LENGTH_MISMATCH | STATUS_BUFFER_TOO_SMALL
            )
        {
            return Err(format!(
                "NtQueryInformationProcess sizing failed with NTSTATUS 0x{:08X}",
                status as u32
            ));
        }
        if required < size_of::<UnicodeString>() as u32 {
            return Err(format!(
                "NtQueryInformationProcess reported an invalid command-line buffer size {required}"
            ));
        }

        for _ in 0..MAX_NATIVE_QUERY_RESIZES {
            let word_size = size_of::<usize>();
            let word_count = (required as usize)
                .checked_add(word_size - 1)
                .ok_or_else(|| "command-line buffer size overflow".to_string())?
                / word_size;
            let mut buffer = vec![0_usize; word_count];
            let byte_capacity = buffer
                .len()
                .checked_mul(word_size)
                .ok_or_else(|| "command-line buffer capacity overflow".to_string())?;
            let byte_capacity = u32::try_from(byte_capacity)
                .map_err(|_| "command-line buffer exceeds the Windows API limit".to_string())?;
            let mut returned = byte_capacity;
            let status = unsafe {
                NtQueryInformationProcess(
                    handle,
                    PROCESS_COMMAND_LINE_INFORMATION,
                    buffer.as_mut_ptr().cast(),
                    byte_capacity,
                    &mut returned,
                )
            };
            if status >= 0 {
                let valid_length = if returned == 0 {
                    byte_capacity
                } else {
                    returned
                };
                if valid_length > byte_capacity {
                    return Err(format!(
                        "NtQueryInformationProcess returned {valid_length} bytes into a {byte_capacity}-byte buffer"
                    ));
                }
                return parse_process_command_line_buffer(&buffer, valid_length as usize);
            }
            if matches!(
                status,
                STATUS_INFO_LENGTH_MISMATCH | STATUS_BUFFER_TOO_SMALL
            ) && returned > byte_capacity
            {
                required = returned;
                continue;
            }
            return Err(format!(
                "NtQueryInformationProcess failed with NTSTATUS 0x{:08X}",
                status as u32
            ));
        }
        Err("NtQueryInformationProcess command-line size changed repeatedly".to_string())
    }

    fn parse_process_command_line_buffer(
        buffer: &[usize],
        valid_length: usize,
    ) -> Result<String, String> {
        let capacity = buffer
            .len()
            .checked_mul(size_of::<usize>())
            .ok_or_else(|| "command-line buffer capacity overflow".to_string())?;
        if valid_length < size_of::<UnicodeString>() || valid_length > capacity {
            return Err(format!(
                "invalid command-line response length {valid_length} for {capacity}-byte buffer"
            ));
        }
        let descriptor = unsafe { ptr::read_unaligned(buffer.as_ptr().cast::<UnicodeString>()) };
        if descriptor.length > descriptor.maximum_length {
            return Err("command-line UNICODE_STRING length exceeds maximum length".to_string());
        }
        if !descriptor.length.is_multiple_of(2) {
            return Err("command-line UNICODE_STRING has an odd byte length".to_string());
        }
        if descriptor.length == 0 {
            return Ok(String::new());
        }
        if descriptor.buffer.is_null() {
            return Err("command-line UNICODE_STRING has a null buffer".to_string());
        }

        let base = buffer.as_ptr() as usize;
        let valid_end = base
            .checked_add(valid_length)
            .ok_or_else(|| "command-line response address overflow".to_string())?;
        let string_start = descriptor.buffer as usize;
        let string_end = string_start
            .checked_add(descriptor.length as usize)
            .ok_or_else(|| "command-line string address overflow".to_string())?;
        if string_start < base || string_end > valid_end {
            return Err(
                "command-line UNICODE_STRING points outside its response buffer".to_string(),
            );
        }

        let unit_count = descriptor.length as usize / 2;
        let units = (0..unit_count)
            .map(|index| unsafe { ptr::read_unaligned(descriptor.buffer.add(index)) })
            .collect::<Vec<_>>();
        String::from_utf16(&units)
            .map_err(|error| format!("command-line UNICODE_STRING is invalid UTF-16: {error}"))
    }

    pub(super) fn loopback_listener_state(pid: u32, port: u16) -> ListenerState {
        // The managed endpoint is exactly IPv4 127.0.0.1. Exact IPv6 ::1 is
        // deliberately not claimed as ownership coverage for that endpoint,
        // but an IPv6 wildcard may accept IPv4-mapped traffic when dual-stack
        // and therefore remains non-exclusive, non-authorizing state.
        listener_state_from_owner_queries(
            pid,
            query_listener_owners(AF_INET, port),
            query_listener_owners(AF_INET6, port),
        )
    }

    fn listener_state_from_owner_queries(
        pid: u32,
        ipv4: Result<RelevantListenerOwners, String>,
        ipv6: Result<RelevantListenerOwners, String>,
    ) -> ListenerState {
        match (ipv4, ipv6) {
            (Ok(mut ipv4), Ok(ipv6)) => {
                ipv4.extend(ipv6);
                listener_state_from_owners(pid, ipv4)
            }
            (Err(ipv4), Ok(_)) => ListenerState::Uninspectable(ipv4),
            (Ok(_), Err(ipv6)) => ListenerState::Uninspectable(ipv6),
            (Err(ipv4), Err(ipv6)) => ListenerState::Uninspectable(format!(
                "IPv4 listener inspection failed: {ipv4}; IPv6 listener inspection failed: {ipv6}"
            )),
        }
    }

    fn query_listener_owners(
        address_family: u32,
        port: u16,
    ) -> Result<RelevantListenerOwners, String> {
        let family_name = match address_family {
            AF_INET => "IPv4",
            AF_INET6 => "IPv6",
            _ => return Err(format!("unsupported TCP address family {address_family}")),
        };
        let mut required = 0_u32;
        let status = unsafe {
            GetExtendedTcpTable(
                ptr::null_mut(),
                &mut required,
                0,
                address_family,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        if status != 0 && status != ERROR_INSUFFICIENT_BUFFER {
            return Err(format!(
                "GetExtendedTcpTable {family_name} sizing failed with Windows error {status}"
            ));
        }
        if required < size_of::<u32>() as u32 {
            if status == 0 {
                return Ok(RelevantListenerOwners::default());
            }
            return Err(format!(
                "GetExtendedTcpTable {family_name} reported an invalid table size {required}"
            ));
        }

        for _ in 0..MAX_NATIVE_QUERY_RESIZES {
            let word_size = size_of::<usize>();
            let word_count = (required as usize)
                .checked_add(word_size - 1)
                .ok_or_else(|| "TCP table buffer size overflow".to_string())?
                / word_size;
            let mut buffer = vec![0_usize; word_count];
            let byte_capacity = buffer
                .len()
                .checked_mul(word_size)
                .ok_or_else(|| "TCP table buffer capacity overflow".to_string())?;
            let byte_capacity = u32::try_from(byte_capacity)
                .map_err(|_| "TCP table buffer exceeds the Windows API limit".to_string())?;
            let mut returned = byte_capacity;
            let status = unsafe {
                GetExtendedTcpTable(
                    buffer.as_mut_ptr().cast(),
                    &mut returned,
                    0,
                    address_family,
                    TCP_TABLE_OWNER_PID_LISTENER,
                    0,
                )
            };
            if status == 0 {
                if returned > byte_capacity {
                    return Err(format!(
                        "GetExtendedTcpTable {family_name} returned {returned} bytes into a {byte_capacity}-byte buffer"
                    ));
                }
                let bytes = unsafe {
                    std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), returned as usize)
                };
                return match address_family {
                    AF_INET => parse_ipv4_relevant_listener_owners(bytes, port),
                    AF_INET6 => parse_ipv6_relevant_listener_owners(bytes, port),
                    _ => unreachable!("address family was validated above"),
                };
            }
            if status == ERROR_INSUFFICIENT_BUFFER && returned > byte_capacity {
                required = returned;
                continue;
            }
            return Err(format!(
                "GetExtendedTcpTable {family_name} failed with Windows error {status}"
            ));
        }
        Err(format!(
            "GetExtendedTcpTable {family_name} size changed repeatedly"
        ))
    }

    fn parse_ipv4_relevant_listener_owners(
        table: &[u8],
        port: u16,
    ) -> Result<RelevantListenerOwners, String> {
        if table.len() < size_of::<u32>() {
            return Err("IPv4 TCP owner table is shorter than its entry count".to_string());
        }
        let count = unsafe { ptr::read_unaligned(table.as_ptr().cast::<u32>()) } as usize;
        let rows_size = count
            .checked_mul(size_of::<MibTcpRowOwnerPid>())
            .ok_or_else(|| "IPv4 TCP owner row count overflow".to_string())?;
        let required = size_of::<u32>()
            .checked_add(rows_size)
            .ok_or_else(|| "IPv4 TCP owner table size overflow".to_string())?;
        if required > table.len() {
            return Err(format!(
                "IPv4 TCP owner table declares {count} rows but contains only {} bytes",
                table.len()
            ));
        }

        let loopback = u32::from_ne_bytes([127, 0, 0, 1]);
        let wildcard = u32::from_ne_bytes([0, 0, 0, 0]);
        let mut owners = RelevantListenerOwners::default();
        for index in 0..count {
            let offset = size_of::<u32>() + index * size_of::<MibTcpRowOwnerPid>();
            let row = unsafe {
                ptr::read_unaligned(table.as_ptr().add(offset).cast::<MibTcpRowOwnerPid>())
            };
            if row.state != MIB_TCP_STATE_LISTEN || u16::from_be(row.local_port as u16) != port {
                continue;
            }
            if row.local_address == loopback {
                owners.loopback.push(row.owning_pid);
            } else if row.local_address == wildcard {
                owners.wildcard.push(row.owning_pid);
            }
        }
        owners.normalize();
        Ok(owners)
    }

    fn parse_ipv6_relevant_listener_owners(
        table: &[u8],
        port: u16,
    ) -> Result<RelevantListenerOwners, String> {
        if table.len() < size_of::<u32>() {
            return Err("IPv6 TCP owner table is shorter than its entry count".to_string());
        }
        let count = unsafe { ptr::read_unaligned(table.as_ptr().cast::<u32>()) } as usize;
        let rows_size = count
            .checked_mul(size_of::<MibTcp6RowOwnerPid>())
            .ok_or_else(|| "IPv6 TCP owner row count overflow".to_string())?;
        let required = size_of::<u32>()
            .checked_add(rows_size)
            .ok_or_else(|| "IPv6 TCP owner table size overflow".to_string())?;
        if required > table.len() {
            return Err(format!(
                "IPv6 TCP owner table declares {count} rows but contains only {} bytes",
                table.len()
            ));
        }

        let wildcard = [0_u8; 16];
        let mut loopback = [0_u8; 16];
        loopback[15] = 1;
        let mut owners = RelevantListenerOwners::default();
        for index in 0..count {
            let offset = size_of::<u32>() + index * size_of::<MibTcp6RowOwnerPid>();
            let row = unsafe {
                ptr::read_unaligned(table.as_ptr().add(offset).cast::<MibTcp6RowOwnerPid>())
            };
            if row.state != MIB_TCP_STATE_LISTEN || u16::from_be(row.local_port as u16) != port {
                continue;
            }
            // The managed endpoint is exactly IPv4 127.0.0.1. IPv6 ::1 on
            // the same numeric port cannot accept that endpoint, while an
            // IPv6 wildcard may accept IPv4-mapped traffic depending on the
            // socket's v6-only setting and must therefore remain relevant.
            if row.local_address == wildcard {
                owners.wildcard.push(row.owning_pid);
            } else if row.local_address == loopback {
                owners.unsupported_ipv6_loopback.push(row.owning_pid);
            }
        }
        owners.normalize();
        Ok(owners)
    }

    fn listener_state_from_owners(pid: u32, owners: RelevantListenerOwners) -> ListenerState {
        if !owners.unsupported_ipv6_loopback.is_empty() {
            return ListenerState::Uninspectable(format!(
                "exact IPv6 loopback (::1) listener ownership is unsupported for the IPv4-only managed endpoint; observed owner PIDs {:?}",
                owners.unsupported_ipv6_loopback
            ));
        }
        let owners_all = owners.all();
        if owners_all.is_empty() {
            return ListenerState::Absent;
        }
        if owners_all.iter().any(|owner| *owner != pid) {
            return ListenerState::OwnedByOther(owners_all);
        }
        if owners.wildcard.contains(&pid) {
            ListenerState::OwnedByTargetWildcard
        } else {
            ListenerState::OwnedByTarget
        }
    }

    fn format_windows_start_token(filetime: u64) -> String {
        format!("windows-filetime:{filetime}")
    }

    fn duration_to_windows_millis(duration: Duration) -> u32 {
        if duration.is_zero() {
            return 0;
        }
        let partial_millisecond = if duration.subsec_nanos().is_multiple_of(1_000_000) {
            0
        } else {
            1
        };
        let rounded_up = duration.as_millis() + partial_millisecond;
        rounded_up.min(u128::from(u32::MAX - 1)) as u32
    }

    fn split_windows_command_line(command_line: &str) -> Vec<String> {
        let chars = command_line.chars().collect::<Vec<_>>();
        let mut argv = Vec::new();
        let mut index = 0;
        while index < chars.len() {
            while index < chars.len() && chars[index].is_whitespace() {
                index += 1;
            }
            if index == chars.len() {
                break;
            }
            let mut argument = String::new();
            let mut quoted = false;
            while index < chars.len() {
                if !quoted && chars[index].is_whitespace() {
                    break;
                }
                let mut backslashes = 0;
                while index < chars.len() && chars[index] == '\\' {
                    backslashes += 1;
                    index += 1;
                }
                if index < chars.len() && chars[index] == '"' {
                    argument.extend(std::iter::repeat_n('\\', backslashes / 2));
                    if backslashes % 2 == 0 {
                        quoted = !quoted;
                    } else {
                        argument.push('"');
                    }
                    index += 1;
                } else {
                    argument.extend(std::iter::repeat_n('\\', backslashes));
                    if index < chars.len() {
                        argument.push(chars[index]);
                        index += 1;
                    }
                }
            }
            argv.push(argument);
        }
        argv
    }

    #[cfg(test)]
    mod tests {
        use std::fs;
        use std::net::{Ipv4Addr, Ipv6Addr, TcpListener};
        use std::path::{Path, PathBuf};
        use std::process::{Command, Stdio};
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::thread;
        use std::time::Instant;

        use super::*;

        const NATIVE_SMOKE_HELPER_ENV: &str = "FERRIC_TEST_PROCESS_SMOKE_HELPER";
        const NATIVE_SMOKE_MODE_ENV: &str = "FERRIC_TEST_PROCESS_SMOKE_MODE";
        const NATIVE_SMOKE_READY_ENV: &str = "FERRIC_TEST_PROCESS_SMOKE_READY";
        const NATIVE_SMOKE_TIMEOUT: Duration = Duration::from_secs(15);
        static NEXT_READY_MARKER: AtomicU64 = AtomicU64::new(0);

        struct ReadyMarker(PathBuf);

        impl ReadyMarker {
            fn new(mode: &str) -> Self {
                let sequence = NEXT_READY_MARKER.fetch_add(1, Ordering::Relaxed);
                Self(std::env::temp_dir().join(format!(
                    "ferric-process-smoke-{}-{sequence}-{mode}.ready",
                    std::process::id()
                )))
            }
        }

        impl Drop for ReadyMarker {
            fn drop(&mut self) {
                let _ = fs::remove_file(&self.0);
                let _ = fs::remove_file(staged_ready_path(&self.0));
            }
        }

        fn staged_ready_path(path: &Path) -> PathBuf {
            path.with_extension("ready.pending")
        }

        fn publish_ready(path: &Path, port: u16) {
            let pid = std::process::id();
            let staged = staged_ready_path(path);
            fs::write(&staged, format!("ferric-native-smoke-v1 {pid} {port}\n"))
                .expect("stage complete native smoke readiness coordinate");
            fs::rename(&staged, path).expect("atomically publish native smoke readiness");
        }

        fn parse_ready(value: &str, expected_pid: u32) -> Option<u16> {
            let mut fields = value.split_whitespace();
            let version = fields.next()?;
            let pid = fields.next()?.parse::<u32>().ok()?;
            let port = fields.next()?.parse::<u16>().ok()?;
            if version != "ferric-native-smoke-v1"
                || pid != expected_pid
                || port == 0
                || fields.next().is_some()
            {
                return None;
            }
            Some(port)
        }

        #[test]
        fn native_smoke_ready_marker_requires_complete_matching_coordinate() {
            assert_eq!(
                parse_ready("ferric-native-smoke-v1 42 8080\n", 42),
                Some(8080)
            );
            for invalid in [
                "",
                "ferric-native-smoke-v1",
                "ferric-native-smoke-v1 42",
                "ferric-native-smoke-v1 42 0",
                "ferric-native-smoke-v1 41 8080",
                "ferric-native-smoke-v1 42 8080 trailing",
                "ferric-native-smoke-v2 42 8080",
            ] {
                assert_eq!(parse_ready(invalid, 42), None, "accepted {invalid:?}");
            }
        }

        struct RetainedSmokeGuard {
            process: Option<LiveProcess>,
            child: Option<crate::test_process_containment::ContainedChild>,
            _ready: ReadyMarker,
        }

        impl RetainedSmokeGuard {
            fn process(&self) -> &LiveProcess {
                self.process
                    .as_ref()
                    .expect("smoke process remains retained")
            }
        }

        impl Drop for RetainedSmokeGuard {
            fn drop(&mut self) {
                if let Some(process) = self.process.take() {
                    let cleanup = process.terminate().and_then(|_| {
                        if process.wait(Duration::from_secs(5))? {
                            Ok(())
                        } else {
                            Err(ProcessError::Operation(format!(
                                "retained Windows native-smoke process {} did not exit within 5s",
                                process.pid()
                            )))
                        }
                    });
                    if let Err(error) = cleanup {
                        crate::test_process_containment::abort_on_cleanup_failure(
                            "could not terminate retained Windows native-smoke process",
                            error,
                        );
                    }
                }
                if let Some(mut child) = self.child.take()
                    && let Err(error) = child.terminate_and_reap()
                {
                    crate::test_process_containment::abort_on_cleanup_failure(
                        "could not clean Windows native-smoke process tree",
                        error,
                    );
                }
            }
        }

        fn wait_for_ready(
            child: &mut crate::test_process_containment::ContainedChild,
            path: &Path,
        ) -> u16 {
            let deadline = Instant::now() + NATIVE_SMOKE_TIMEOUT;
            loop {
                if let Ok(value) = fs::read_to_string(path)
                    && let Some(port) = parse_ready(&value, child.child().id())
                {
                    return port;
                }
                if let Some(status) = child
                    .try_wait_leader()
                    .expect("inspect smoke helper status")
                {
                    panic!("native smoke helper exited before readiness: {status}");
                }
                assert!(
                    Instant::now() < deadline,
                    "native smoke helper did not publish readiness within {NATIVE_SMOKE_TIMEOUT:?}"
                );
                thread::sleep(Duration::from_millis(10));
            }
        }

        fn spawn_retained_smoke_child(mode: &str) -> (RetainedSmokeGuard, u16) {
            crate::test_process_containment::ensure_current_process_tree_is_contained()
                .expect("install Windows native-smoke process containment");
            let ready = ReadyMarker::new(mode);
            let mut command =
                Command::new(std::env::current_exe().expect("current test executable"));
            command
                .args([
                    "--exact",
                    "server_process::platform::tests::native_process_smoke_helper",
                    "--nocapture",
                ])
                .env(NATIVE_SMOKE_HELPER_ENV, "1")
                .env(NATIVE_SMOKE_MODE_ENV, mode)
                .env(NATIVE_SMOKE_READY_ENV, &ready.0)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            let mut child = crate::test_process_containment::ContainedChild::spawn(&mut command)
                .expect("spawn contained native process smoke helper");
            let port = wait_for_ready(&mut child, &ready.0);
            let source_handle = child.child().as_raw_handle() as Handle;
            let process = LiveProcess::acquire_child(child.child())
                .expect("duplicate the spawned child's exact process HANDLE");
            assert_ne!(
                process.handle, source_handle,
                "acquire_child must retain a distinct duplicated HANDLE"
            );

            (
                RetainedSmokeGuard {
                    process: Some(process),
                    child: Some(child),
                    _ready: ready,
                },
                port,
            )
        }

        fn wait_for_listener_state(
            process: &LiveProcess,
            port: u16,
            expected: &ListenerState,
        ) -> ListenerState {
            let deadline = Instant::now() + NATIVE_SMOKE_TIMEOUT;
            loop {
                let state = loopback_listener_state(process.pid(), port);
                if &state == expected {
                    return state;
                }
                assert!(
                    !process.wait(Duration::ZERO).expect("poll retained HANDLE"),
                    "smoke helper exited while waiting for listener state {expected:?}; last state: {state:?}"
                );
                assert!(
                    Instant::now() < deadline,
                    "listener state did not become {expected:?}; last state: {state:?}"
                );
                thread::sleep(Duration::from_millis(10));
            }
        }

        fn filetime_for_handle(handle: Handle) -> u64 {
            let mut creation = MaybeUninit::<FileTime>::uninit();
            let mut exit = MaybeUninit::<FileTime>::uninit();
            let mut kernel = MaybeUninit::<FileTime>::uninit();
            let mut user = MaybeUninit::<FileTime>::uninit();
            assert_ne!(
                unsafe {
                    GetProcessTimes(
                        handle,
                        creation.as_mut_ptr(),
                        exit.as_mut_ptr(),
                        kernel.as_mut_ptr(),
                        user.as_mut_ptr(),
                    )
                },
                0,
                "read FILETIME from duplicated process HANDLE"
            );
            let creation = unsafe { creation.assume_init() };
            (u64::from(creation.high_date_time) << 32) | u64::from(creation.low_date_time)
        }

        /// A normal harness invocation returns immediately. The parent smoke
        /// explicitly re-runs only this test with one harmless socket mode.
        #[test]
        fn native_process_smoke_helper() {
            if std::env::var(NATIVE_SMOKE_HELPER_ENV).ok().as_deref() != Some("1") {
                return;
            }
            crate::test_process_containment::arm_current_process_parent_death_signal()
                .expect("arm native-smoke parent-death containment");
            let mode = std::env::var(NATIVE_SMOKE_MODE_ENV).expect("native smoke mode");
            let ready = PathBuf::from(
                std::env::var_os(NATIVE_SMOKE_READY_ENV).expect("native smoke ready path"),
            );
            let listener = match mode.as_str() {
                "ipv4-loopback" => TcpListener::bind((Ipv4Addr::LOCALHOST, 0)),
                "ipv4-wildcard" => TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)),
                "ipv6-loopback" => TcpListener::bind((Ipv6Addr::LOCALHOST, 0)),
                other => panic!("unknown native smoke mode {other:?}"),
            }
            .expect("bind native smoke listener");
            publish_ready(&ready, listener.local_addr().unwrap().port());
            thread::sleep(Duration::from_secs(60));
        }

        fn process_command_line_buffer(command_line: &str) -> (Vec<usize>, usize) {
            let wide = command_line.encode_utf16().collect::<Vec<_>>();
            let byte_length = wide.len() * size_of::<u16>();
            let valid_length = size_of::<UnicodeString>() + byte_length;
            let word_count = valid_length.div_ceil(size_of::<usize>());
            let mut buffer = vec![0_usize; word_count];
            let string = unsafe {
                buffer
                    .as_mut_ptr()
                    .cast::<u8>()
                    .add(size_of::<UnicodeString>())
                    .cast::<u16>()
            };
            for (index, unit) in wide.into_iter().enumerate() {
                unsafe { ptr::write_unaligned(string.add(index), unit) };
            }
            let descriptor = UnicodeString {
                length: byte_length as u16,
                maximum_length: byte_length as u16,
                buffer: string,
            };
            unsafe {
                ptr::write_unaligned(buffer.as_mut_ptr().cast::<UnicodeString>(), descriptor)
            };
            (buffer, valid_length)
        }

        fn ipv4_tcp_table(rows: &[[u32; 6]]) -> Vec<u8> {
            let mut table =
                Vec::with_capacity(size_of::<u32>() + rows.len() * size_of::<MibTcpRowOwnerPid>());
            table.extend_from_slice(&(rows.len() as u32).to_ne_bytes());
            for row in rows {
                for field in row {
                    table.extend_from_slice(&field.to_ne_bytes());
                }
            }
            table
        }

        fn ipv6_tcp_table(rows: &[MibTcp6RowOwnerPid]) -> Vec<u8> {
            let mut table = Vec::with_capacity(size_of::<u32>() + std::mem::size_of_val(rows));
            table.extend_from_slice(&(rows.len() as u32).to_ne_bytes());
            for row in rows {
                table.extend_from_slice(&row.local_address);
                table.extend_from_slice(&row.local_scope_id.to_ne_bytes());
                table.extend_from_slice(&row.local_port.to_ne_bytes());
                table.extend_from_slice(&row.remote_address);
                table.extend_from_slice(&row.remote_scope_id.to_ne_bytes());
                table.extend_from_slice(&row.remote_port.to_ne_bytes());
                table.extend_from_slice(&row.state.to_ne_bytes());
                table.extend_from_slice(&row.owning_pid.to_ne_bytes());
            }
            table
        }

        #[test]
        fn windows_start_token_uses_decimal_filetime() {
            assert_eq!(
                format_windows_start_token(133_999_123_456_789_012),
                "windows-filetime:133999123456789012"
            );
        }

        #[test]
        fn windows_command_line_parser_preserves_quoted_argv() {
            assert_eq!(
                split_windows_command_line(
                    r#""C:\Program Files\llama-server.exe" -m "models\example model.gguf" -c 8192 --seed 42"#,
                ),
                [
                    r#"C:\Program Files\llama-server.exe"#,
                    "-m",
                    r#"models\example model.gguf"#,
                    "-c",
                    "8192",
                    "--seed",
                    "42",
                ]
            );
        }

        #[test]
        fn native_command_line_response_parser_validates_unicode_string_bounds() {
            let expected = r#""C:\Program Files\llama-server.exe" --port 8080"#;
            let (buffer, valid_length) = process_command_line_buffer(expected);
            assert_eq!(
                parse_process_command_line_buffer(&buffer, valid_length).unwrap(),
                expected
            );
            assert!(
                parse_process_command_line_buffer(&buffer, size_of::<UnicodeString>() - 1)
                    .unwrap_err()
                    .contains("invalid command-line response length")
            );
        }

        #[test]
        fn ipv4_tcp_owner_parser_includes_loopback_and_wildcard_on_exact_port() {
            let loopback = u32::from_ne_bytes([127, 0, 0, 1]);
            let wildcard = u32::from_ne_bytes([0, 0, 0, 0]);
            let port_8080 = u32::from(8080_u16.to_be());
            let port_8081 = u32::from(8081_u16.to_be());
            let table = ipv4_tcp_table(&[
                [MIB_TCP_STATE_LISTEN, loopback, port_8080, 0, 0, 10],
                [MIB_TCP_STATE_LISTEN, loopback, port_8080, 0, 0, 20],
                [MIB_TCP_STATE_LISTEN, loopback, port_8080, 0, 0, 20],
                [MIB_TCP_STATE_LISTEN, wildcard, port_8080, 0, 0, 30],
                [MIB_TCP_STATE_LISTEN, loopback, port_8081, 0, 0, 40],
                [5, loopback, port_8080, 0, 0, 50],
            ]);
            assert_eq!(
                parse_ipv4_relevant_listener_owners(&table, 8080).unwrap(),
                RelevantListenerOwners {
                    loopback: vec![10, 20],
                    wildcard: vec![30],
                    unsupported_ipv6_loopback: vec![],
                }
            );
            assert!(parse_ipv4_relevant_listener_owners(&table[..8], 8080).is_err());
        }

        #[test]
        fn ipv6_tcp_owner_parser_records_unsupported_loopback_and_wildcard() {
            let mut loopback = [0_u8; 16];
            loopback[15] = 1;
            let wildcard = [0_u8; 16];
            let port_8080 = u32::from(8080_u16.to_be());
            let port_8081 = u32::from(8081_u16.to_be());
            let row = |address, port, state, owning_pid| MibTcp6RowOwnerPid {
                local_address: address,
                local_scope_id: 0,
                local_port: port,
                remote_address: wildcard,
                remote_scope_id: 0,
                remote_port: 0,
                state,
                owning_pid,
            };
            let table = ipv6_tcp_table(&[
                row(loopback, port_8080, MIB_TCP_STATE_LISTEN, 10),
                row(loopback, port_8080, MIB_TCP_STATE_LISTEN, 10),
                row(wildcard, port_8080, MIB_TCP_STATE_LISTEN, 20),
                row(loopback, port_8081, MIB_TCP_STATE_LISTEN, 30),
                row(loopback, port_8080, 5, 40),
            ]);
            assert_eq!(
                parse_ipv6_relevant_listener_owners(&table, 8080).unwrap(),
                RelevantListenerOwners {
                    loopback: vec![],
                    wildcard: vec![20],
                    unsupported_ipv6_loopback: vec![10],
                }
            );
            assert!(parse_ipv6_relevant_listener_owners(&table[..8], 8080).is_err());
        }

        #[test]
        fn loopback_listener_ownership_matrix() {
            let rows = [
                (
                    "exclusive IPv4 loopback",
                    Ok(RelevantListenerOwners {
                        loopback: vec![10],
                        wildcard: vec![],
                        unsupported_ipv6_loopback: vec![],
                    }),
                    Ok(RelevantListenerOwners::default()),
                    ListenerState::OwnedByTarget,
                    true,
                ),
                (
                    "absent",
                    Ok(RelevantListenerOwners::default()),
                    Ok(RelevantListenerOwners::default()),
                    ListenerState::Absent,
                    true,
                ),
                (
                    "foreign IPv4 loopback",
                    Ok(RelevantListenerOwners {
                        loopback: vec![20],
                        wildcard: vec![],
                        unsupported_ipv6_loopback: vec![],
                    }),
                    Ok(RelevantListenerOwners::default()),
                    ListenerState::OwnedByOther(vec![20]),
                    false,
                ),
                (
                    "multiple foreign IPv4 loopback owners",
                    Ok(RelevantListenerOwners {
                        loopback: vec![30, 20, 30],
                        wildcard: vec![],
                        unsupported_ipv6_loopback: vec![],
                    }),
                    Ok(RelevantListenerOwners::default()),
                    ListenerState::OwnedByOther(vec![20, 30]),
                    false,
                ),
                (
                    "target and foreign shared IPv4 loopback",
                    Ok(RelevantListenerOwners {
                        loopback: vec![20, 10, 20],
                        wildcard: vec![],
                        unsupported_ipv6_loopback: vec![],
                    }),
                    Ok(RelevantListenerOwners::default()),
                    ListenerState::OwnedByOther(vec![10, 20]),
                    false,
                ),
                (
                    "IPv4 wildcard",
                    Ok(RelevantListenerOwners {
                        loopback: vec![],
                        wildcard: vec![10],
                        unsupported_ipv6_loopback: vec![],
                    }),
                    Ok(RelevantListenerOwners::default()),
                    ListenerState::OwnedByTargetWildcard,
                    false,
                ),
                (
                    "IPv6 wildcard or dual-stack",
                    Ok(RelevantListenerOwners::default()),
                    Ok(RelevantListenerOwners {
                        loopback: vec![],
                        wildcard: vec![10],
                        unsupported_ipv6_loopback: vec![],
                    }),
                    ListenerState::OwnedByTargetWildcard,
                    false,
                ),
                (
                    "exact IPv6 loopback is explicitly unsupported",
                    Ok(RelevantListenerOwners::default()),
                    Ok(RelevantListenerOwners {
                        loopback: vec![],
                        wildcard: vec![],
                        unsupported_ipv6_loopback: vec![10],
                    }),
                    ListenerState::Uninspectable(
                        "exact IPv6 loopback (::1) listener ownership is unsupported for the IPv4-only managed endpoint; observed owner PIDs [10]"
                            .to_string(),
                    ),
                    false,
                ),
                (
                    "uninspectable IPv4 table",
                    Err("IPv4 owner query denied".to_string()),
                    Ok(RelevantListenerOwners::default()),
                    ListenerState::Uninspectable("IPv4 owner query denied".to_string()),
                    false,
                ),
                (
                    "uninspectable IPv6 table",
                    Ok(RelevantListenerOwners::default()),
                    Err("IPv6 owner query denied".to_string()),
                    ListenerState::Uninspectable("IPv6 owner query denied".to_string()),
                    false,
                ),
            ];

            for (name, ipv4, ipv6, expected, authorizing) in rows {
                let state = listener_state_from_owner_queries(10, ipv4, ipv6);
                assert_eq!(state, expected, "row {name}");
                assert_eq!(state.permits_teardown(), authorizing, "row {name}");
            }
        }

        #[test]
        fn native_low_privilege_smoke_inspects_current_process_and_listener() {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            let port = listener.local_addr().unwrap().port();
            let process = LiveProcess::acquire(std::process::id()).unwrap();
            let facts = process.inspect(port).unwrap();
            assert!(!facts.identity.start_token.is_empty());
            assert!(!facts.identity.executable.as_os_str().is_empty());
            assert!(!facts.identity.argv.is_empty());
            assert_eq!(facts.listener, ListenerState::OwnedByTarget);
            assert_eq!(
                loopback_listener_state(std::process::id(), port),
                ListenerState::OwnedByTarget
            );

            let wildcard = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
            let wildcard_port = wildcard.local_addr().unwrap().port();
            assert_eq!(
                loopback_listener_state(std::process::id(), wildcard_port),
                ListenerState::OwnedByTargetWildcard
            );
        }

        #[test]
        fn windows_retained_process_handle_smoke() {
            let (mut loopback, loopback_port) = spawn_retained_smoke_child("ipv4-loopback");
            let process = loopback.process();
            assert_eq!(
                wait_for_listener_state(process, loopback_port, &ListenerState::OwnedByTarget),
                ListenerState::OwnedByTarget
            );
            let filetime = filetime_for_handle(process.handle);
            assert!(filetime > 0, "process creation FILETIME must be positive");
            let facts = process
                .inspect(loopback_port)
                .expect("inspect exact retained process HANDLE");
            assert_eq!(
                facts.identity.start_token,
                format_windows_start_token(filetime)
            );
            validate_start_token(&facts.identity.start_token)
                .expect("native FILETIME token must be canonical");
            assert!(!facts.identity.executable.as_os_str().is_empty());
            assert!(!facts.identity.argv.is_empty());
            assert_eq!(facts.listener, ListenerState::OwnedByTarget);

            // Take the retained duplicate out of its cleanup guard. The
            // contained child remains owned solely as the bounded process-tree
            // reap authority; termination and process-exit observation below
            // still target only the retained duplicate HANDLE.
            let retained = loopback
                .process
                .take()
                .expect("retained loopback process HANDLE");
            assert!(
                retained
                    .terminate()
                    .expect("terminate through retained HANDLE")
            );
            assert!(
                retained
                    .wait(Duration::from_secs(5))
                    .expect("wait through retained HANDLE")
            );
            let duplicated_handle = retained.handle;
            let mut handle_flags = 0_u32;
            assert_ne!(
                unsafe { GetHandleInformation(duplicated_handle, &mut handle_flags) },
                0,
                "duplicated process HANDLE must remain valid while retained"
            );
            retained
                .release_for_test()
                .expect("the LiveProcess drop path must release its duplicated HANDLE");
            loopback
                .child
                .as_mut()
                .expect("contained smoke child remains retained")
                .terminate_and_reap()
                .expect("reap the exact-terminated Windows smoke process tree");

            let (wildcard, wildcard_port) = spawn_retained_smoke_child("ipv4-wildcard");
            let wildcard_state = wait_for_listener_state(
                wildcard.process(),
                wildcard_port,
                &ListenerState::OwnedByTargetWildcard,
            );
            assert_eq!(wildcard_state, ListenerState::OwnedByTargetWildcard);
            assert!(!wildcard_state.permits_teardown());
            drop(wildcard);

            let (ipv6_loopback, ipv6_loopback_port) = spawn_retained_smoke_child("ipv6-loopback");
            let expected_ipv6 = ListenerState::Uninspectable(format!(
                "exact IPv6 loopback (::1) listener ownership is unsupported for the IPv4-only managed endpoint; observed owner PIDs [{}]",
                ipv6_loopback.process().pid()
            ));
            let ipv6_state = wait_for_listener_state(
                ipv6_loopback.process(),
                ipv6_loopback_port,
                &expected_ipv6,
            );
            assert_eq!(ipv6_state, expected_ipv6);
            assert!(
                !ipv6_state.permits_teardown(),
                "an exact ::1 observation must never authorize the IPv4-only endpoint"
            );
        }

        #[test]
        fn finite_wait_rounds_up_without_using_infinite_sentinel() {
            assert_eq!(duration_to_windows_millis(Duration::from_nanos(1)), 1);
            assert_eq!(duration_to_windows_millis(Duration::ZERO), 0);
            assert_eq!(
                duration_to_windows_millis(Duration::from_secs(u64::MAX)),
                u32::MAX - 1
            );
        }
    }
}

#[cfg(all(
    target_os = "linux",
    target_endian = "little",
    target_pointer_width = "64",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod platform {
    use std::collections::{BTreeSet, HashSet};
    use std::ffi::{c_int, c_long, c_void};
    use std::fs;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::path::Path;
    use std::time::Instant;

    use super::*;

    // pidfd syscall numbers are shared by Linux's asm-generic architectures
    // and x86_64.  Keeping the raw FFI in this isolated adapter avoids making
    // the default CLI depend on a broad process-management crate.
    const SYS_PIDFD_SEND_SIGNAL: c_long = 424;
    const SYS_PIDFD_OPEN: c_long = 434;
    const SIGKILL: c_int = 9;
    const POLLIN: i16 = 0x0001;
    const EINTR: i32 = 4;
    const ESRCH: i32 = 3;
    const ENOSYS: i32 = 38;

    #[repr(C)]
    struct PollFd {
        fd: c_int,
        events: i16,
        revents: i16,
    }

    unsafe extern "C" {
        fn syscall(number: c_long, ...) -> c_long;
        fn poll(fds: *mut PollFd, nfds: usize, timeout: c_int) -> c_int;
    }

    /// A retained Linux pidfd.  Signals are sent through this descriptor, not
    /// through a numeric PID which may have been recycled after validation.
    pub struct LiveProcess {
        pid: u32,
        pidfd: OwnedFd,
    }

    impl LiveProcess {
        /// Open the pidfd immediately while the original `Child` remains
        /// retained by the caller, which then confirms the child has not
        /// exited before treating the binding as authoritative.
        pub fn acquire_child(child: &Child) -> Result<Self, ProcessError> {
            Self::acquire(child.id())
        }

        pub fn acquire(pid: u32) -> Result<Self, ProcessError> {
            if pid == 0 || pid > i32::MAX as u32 {
                return Err(ProcessError::NotFound(pid));
            }
            let raw = unsafe { syscall(SYS_PIDFD_OPEN, pid as c_int, 0_u32) };
            if raw < 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() == Some(ESRCH) {
                    return Err(ProcessError::NotFound(pid));
                }
                if error.raw_os_error() == Some(ENOSYS) {
                    return Err(ProcessError::Unsupported(
                        "this Linux kernel (pidfd_open unavailable)",
                    ));
                }
                return Err(ProcessError::Operation(format!(
                    "pidfd_open for process {pid}: {error}"
                )));
            }
            let pidfd = unsafe { OwnedFd::from_raw_fd(raw as c_int) };
            Ok(Self { pid, pidfd })
        }

        pub fn pid(&self) -> u32 {
            self.pid
        }

        pub fn inspect(&self, port: u16) -> Result<ProcessFacts, ProcessError> {
            self.require_active()?;
            let boot_id = read_nonempty_trimmed("/proc/sys/kernel/random/boot_id")?;
            let stat = fs::read_to_string(format!("/proc/{}/stat", self.pid)).map_err(|error| {
                map_proc_error(self.pid, format!("read process stat: {error}"), &error)
            })?;
            let start_ticks = parse_proc_start_ticks(&stat).ok_or_else(|| {
                ProcessError::Operation(format!(
                    "parse /proc/{}/stat process start ticks",
                    self.pid
                ))
            })?;
            let executable = fs::read_link(format!("/proc/{}/exe", self.pid)).map_err(|error| {
                map_proc_error(
                    self.pid,
                    format!("read /proc/{}/exe: {error}", self.pid),
                    &error,
                )
            })?;
            let command_line =
                fs::read(format!("/proc/{}/cmdline", self.pid)).map_err(|error| {
                    map_proc_error(
                        self.pid,
                        format!("read /proc/{}/cmdline: {error}", self.pid),
                        &error,
                    )
                })?;
            let argv = parse_linux_argv(self.pid, &command_line)?;
            let listener = loopback_listener_state(self.pid, port);

            // All /proc and socket tables above are PID-indexed.  The pidfd
            // check prevents facts from a replacement PID being paired with
            // the retained process object.
            self.require_active()?;
            Ok(ProcessFacts {
                identity: ProcessIdentity {
                    start_token: format_linux_start_token(&boot_id, start_ticks),
                    executable,
                    argv,
                },
                listener,
            })
        }

        /// Send SIGKILL through the retained pidfd.  `false` means the retained
        /// process had already exited and no numeric PID was signalled.
        pub fn terminate(&self) -> Result<bool, ProcessError> {
            if self.wait(Duration::ZERO)? {
                return Ok(false);
            }
            let result = unsafe {
                syscall(
                    SYS_PIDFD_SEND_SIGNAL,
                    self.pidfd.as_raw_fd(),
                    SIGKILL,
                    std::ptr::null::<c_void>(),
                    0_u32,
                )
            };
            if result < 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() == Some(ESRCH) || self.wait(Duration::ZERO)? {
                    return Ok(false);
                }
                if error.raw_os_error() == Some(ENOSYS) {
                    return Err(ProcessError::Unsupported(
                        "this Linux kernel (pidfd_send_signal unavailable)",
                    ));
                }
                return Err(ProcessError::Operation(format!(
                    "pidfd_send_signal for process {}: {error}",
                    self.pid
                )));
            }
            Ok(true)
        }

        /// Wait at most `timeout` for the retained pidfd to become readable.
        /// Returns `true` when exited and `false` on timeout.
        pub fn wait(&self, timeout: Duration) -> Result<bool, ProcessError> {
            let deadline = Instant::now().checked_add(timeout);
            let mut remaining = timeout;
            loop {
                let mut descriptor = PollFd {
                    fd: self.pidfd.as_raw_fd(),
                    events: POLLIN,
                    revents: 0,
                };
                let result =
                    unsafe { poll(&mut descriptor, 1, duration_to_poll_millis(remaining)) };
                if result > 0 {
                    if descriptor.revents & POLLIN != 0 {
                        return Ok(true);
                    }
                    return Err(ProcessError::Operation(format!(
                        "poll retained pidfd for process {} returned unexpected events {:#x}",
                        self.pid, descriptor.revents
                    )));
                }
                if result == 0 {
                    return Ok(false);
                }
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() != Some(EINTR) {
                    return Err(ProcessError::Operation(format!(
                        "poll retained pidfd for process {}: {error}",
                        self.pid
                    )));
                }
                let Some(deadline) = deadline else {
                    remaining = Duration::from_millis(i32::MAX as u64);
                    continue;
                };
                let now = Instant::now();
                if now >= deadline {
                    return Ok(false);
                }
                remaining = deadline.duration_since(now);
            }
        }

        fn require_active(&self) -> Result<(), ProcessError> {
            if self.wait(Duration::ZERO)? {
                Err(ProcessError::NotFound(self.pid))
            } else {
                Ok(())
            }
        }
    }

    fn parse_linux_argv(pid: u32, command_line: &[u8]) -> Result<Vec<String>, ProcessError> {
        let command_line = command_line.strip_suffix(&[0]).unwrap_or(command_line);
        if command_line.is_empty() {
            return Err(ProcessError::Operation(format!(
                "/proc/{pid}/cmdline contains no argv"
            )));
        }

        command_line
            .split(|byte| *byte == 0)
            .enumerate()
            .map(|(index, argument)| {
                if argument.is_empty() {
                    return Err(ProcessError::Operation(format!(
                        "/proc/{pid}/cmdline argv element {index} is empty"
                    )));
                }
                std::str::from_utf8(argument)
                    .map(str::to_owned)
                    .map_err(|error| {
                        ProcessError::Operation(format!(
                            "/proc/{pid}/cmdline argv element {index} is not UTF-8: {error}"
                        ))
                    })
            })
            .collect()
    }

    pub(super) fn loopback_listener_state(pid: u32, port: u16) -> ListenerState {
        match inspect_loopback_listener_state(pid, port) {
            Ok(state) => state,
            Err(error) => ListenerState::Uninspectable(error),
        }
    }

    fn inspect_loopback_listener_state(pid: u32, port: u16) -> Result<ListenerState, String> {
        let mut relevant_inodes = RelevantListenerInodes::default();
        match fs::read_to_string("/proc/net/tcp") {
            Ok(table) => {
                relevant_inodes.extend(listening_loopback_relevant_inodes(
                    &table,
                    port,
                    AddressFamily::V4,
                )?);
            }
            Err(error) => return Err(format!("read /proc/net/tcp: {error}")),
        }
        match fs::read_to_string("/proc/net/tcp6") {
            Ok(table) => {
                relevant_inodes.extend(listening_loopback_relevant_inodes(
                    &table,
                    port,
                    AddressFamily::V6,
                )?);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("read /proc/net/tcp6: {error}")),
        }
        let listening_inodes = relevant_inodes.all();
        if listening_inodes.is_empty() {
            return Ok(ListenerState::Absent);
        }

        let target_inventory = socket_inodes_for_process(pid);
        let mut peer_inventories = Vec::new();
        let proc_entries = fs::read_dir("/proc").map_err(|error| format!("read /proc: {error}"))?;
        for entry in proc_entries {
            let entry = entry.map_err(|error| {
                format!("enumerate /proc while inspecting listener owners: {error}")
            })?;
            let Some(owner_pid) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
            else {
                continue;
            };
            if owner_pid == pid {
                continue;
            }
            peer_inventories.push((owner_pid, socket_inodes_for_process(owner_pid)));
        }

        listener_state_from_socket_inventories(
            pid,
            port,
            &relevant_inodes,
            target_inventory,
            peer_inventories,
        )
    }

    #[derive(Debug, PartialEq, Eq)]
    enum ProcessSocketInventory {
        Complete(HashSet<String>),
        Vanished,
        Uninspectable(String),
    }

    fn socket_inodes_for_process(pid: u32) -> ProcessSocketInventory {
        let entries = match fs::read_dir(format!("/proc/{pid}/fd")) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return ProcessSocketInventory::Vanished;
            }
            Err(error) => {
                return ProcessSocketInventory::Uninspectable(format!(
                    "read /proc/{pid}/fd: {error}"
                ));
            }
        };
        let mut inodes = HashSet::new();
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return ProcessSocketInventory::Uninspectable(format!(
                        "enumerate /proc/{pid}/fd: {error}"
                    ));
                }
            };
            let target = match fs::read_link(entry.path()) {
                Ok(target) => target,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return ProcessSocketInventory::Uninspectable(format!(
                        "read link {}: {error}",
                        entry.path().display()
                    ));
                }
            };
            if let Some(inode) = socket_inode(&target) {
                inodes.insert(inode.to_string());
            }
        }
        ProcessSocketInventory::Complete(inodes)
    }

    fn listener_state_from_socket_inventories(
        pid: u32,
        port: u16,
        relevant_inodes: &RelevantListenerInodes,
        target_inventory: ProcessSocketInventory,
        peer_inventories: Vec<(u32, ProcessSocketInventory)>,
    ) -> Result<ListenerState, String> {
        let listening_inodes = relevant_inodes.all();
        if listening_inodes.is_empty() {
            return Ok(ListenerState::Absent);
        }
        let target_inodes = match target_inventory {
            ProcessSocketInventory::Complete(inodes) => inodes,
            ProcessSocketInventory::Vanished => HashSet::new(),
            ProcessSocketInventory::Uninspectable(error) => return Err(error),
        };
        let target_loopback = target_inodes
            .intersection(&relevant_inodes.loopback)
            .cloned()
            .collect::<HashSet<_>>();
        let target_wildcard = target_inodes
            .intersection(&relevant_inodes.wildcard)
            .cloned()
            .collect::<HashSet<_>>();
        let target_owns = !target_loopback.is_empty() || !target_wildcard.is_empty();
        let mut accounted_inodes = target_loopback
            .union(&target_wildcard)
            .cloned()
            .collect::<HashSet<_>>();
        let mut other_owners = BTreeSet::new();

        for (owner_pid, inventory) in peer_inventories {
            let inodes = match inventory {
                ProcessSocketInventory::Complete(inodes) => inodes,
                ProcessSocketInventory::Vanished => continue,
                ProcessSocketInventory::Uninspectable(error) => {
                    return Err(format!(
                        "listener owner enumeration is incomplete because PID {owner_pid} is uninspectable: {error}"
                    ));
                }
            };
            let owned_listeners = inodes
                .intersection(&listening_inodes)
                .cloned()
                .collect::<HashSet<_>>();
            if !owned_listeners.is_empty() {
                other_owners.insert(owner_pid);
                accounted_inodes.extend(owned_listeners);
            }
        }

        if accounted_inodes != listening_inodes {
            return Err(format!(
                "loopback port {port} has listening socket inodes whose owners are not inspectable"
            ));
        }
        if !other_owners.is_empty() {
            if target_owns {
                other_owners.insert(pid);
            }
            return Ok(ListenerState::OwnedByOther(
                other_owners.into_iter().collect(),
            ));
        }
        if !target_wildcard.is_empty() {
            Ok(ListenerState::OwnedByTargetWildcard)
        } else if !target_loopback.is_empty() {
            Ok(ListenerState::OwnedByTarget)
        } else {
            Err(format!(
                "loopback port {port} has listening socket inodes but their owning process is not inspectable"
            ))
        }
    }

    fn socket_inode(target: &Path) -> Option<&str> {
        target.to_str()?.strip_prefix("socket:[")?.strip_suffix(']')
    }

    #[derive(Clone, Copy)]
    enum AddressFamily {
        V4,
        V6,
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct RelevantListenerInodes {
        loopback: HashSet<String>,
        wildcard: HashSet<String>,
    }

    impl RelevantListenerInodes {
        fn extend(&mut self, other: Self) {
            self.loopback.extend(other.loopback);
            self.wildcard.extend(other.wildcard);
        }

        fn all(&self) -> HashSet<String> {
            self.loopback.union(&self.wildcard).cloned().collect()
        }
    }

    fn listening_loopback_relevant_inodes(
        table: &str,
        port: u16,
        family: AddressFamily,
    ) -> Result<RelevantListenerInodes, String> {
        let mut inodes = RelevantListenerInodes::default();
        for (index, line) in table.lines().skip(1).enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let fields = line.split_whitespace().collect::<Vec<_>>();
            let Some((address, local_port)) =
                fields.get(1).and_then(|value| value.rsplit_once(':'))
            else {
                return Err(format!(
                    "/proc TCP table row {} has no valid local endpoint",
                    index + 2
                ));
            };
            let address_width = match family {
                AddressFamily::V4 => 8,
                AddressFamily::V6 => 32,
            };
            if address.len() != address_width
                || !address.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(format!(
                    "/proc TCP table row {} has an invalid local address",
                    index + 2
                ));
            }
            let local_port = u16::from_str_radix(local_port, 16).map_err(|_| {
                format!(
                    "/proc TCP table row {} has an invalid local port",
                    index + 2
                )
            })?;
            let state = fields.get(3).ok_or_else(|| {
                format!("/proc TCP table row {} has no connection state", index + 2)
            })?;
            let state = u8::from_str_radix(state, 16).map_err(|_| {
                format!(
                    "/proc TCP table row {} has an invalid connection state",
                    index + 2
                )
            })?;
            if local_port != port || state != 0x0A {
                continue;
            }
            let inode = fields.get(9).ok_or_else(|| {
                format!("/proc TCP listener row {} has no socket inode", index + 2)
            })?;
            let inode_number = inode.parse::<u64>().map_err(|_| {
                format!(
                    "/proc TCP listener row {} has an invalid socket inode",
                    index + 2
                )
            })?;
            if inode_number == 0 {
                return Err(format!(
                    "/proc TCP listener row {} has no inspectable socket inode",
                    index + 2
                ));
            }
            let (loopback, wildcard) = match family {
                AddressFamily::V4 => (
                    address.eq_ignore_ascii_case("0100007F"),
                    address.eq_ignore_ascii_case("00000000"),
                ),
                // IPv6 ::1 cannot accept the exact managed IPv4 endpoint;
                // IPv6 wildcard remains relevant because v4-mapped traffic
                // depends on the socket's v6-only setting.
                AddressFamily::V6 => (
                    false,
                    address.eq_ignore_ascii_case("00000000000000000000000000000000"),
                ),
            };
            if loopback {
                inodes.loopback.insert((*inode).to_string());
            } else if wildcard {
                inodes.wildcard.insert((*inode).to_string());
            }
        }
        Ok(inodes)
    }

    fn parse_proc_start_ticks(stat: &str) -> Option<u64> {
        // Field 2 (`comm`) is parenthesised and may itself contain spaces or
        // ')', so fields must begin after the final closing parenthesis.
        let close = stat.rfind(')')?;
        stat.get(close + 1..)?
            .split_whitespace()
            .nth(19)
            .and_then(|field| field.parse().ok())
    }

    fn format_linux_start_token(boot_id: &str, start_ticks: u64) -> String {
        format!("linux-boot-id:{boot_id};start-ticks:{start_ticks}")
    }

    fn read_nonempty_trimmed(path: &str) -> Result<String, ProcessError> {
        let value = fs::read_to_string(path)
            .map_err(|error| ProcessError::Operation(format!("read {path}: {error}")))?;
        let value = value.trim();
        if value.is_empty() {
            return Err(ProcessError::Operation(format!("{path} was empty")));
        }
        Ok(value.to_string())
    }

    fn map_proc_error(pid: u32, message: String, error: &std::io::Error) -> ProcessError {
        if error.kind() == std::io::ErrorKind::NotFound {
            ProcessError::NotFound(pid)
        } else {
            ProcessError::Operation(message)
        }
    }

    fn duration_to_poll_millis(duration: Duration) -> c_int {
        if duration.is_zero() {
            return 0;
        }
        let partial_millisecond = if duration.subsec_nanos().is_multiple_of(1_000_000) {
            0
        } else {
            1
        };
        let rounded_up = duration.as_millis() + partial_millisecond;
        rounded_up.min(i32::MAX as u128) as c_int
    }

    #[cfg(test)]
    mod tests {
        use std::net::{Ipv4Addr, Ipv6Addr, TcpListener};
        use std::os::unix::process::CommandExt;
        use std::path::{Path, PathBuf};
        use std::process::{Command, Stdio};
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::thread;

        use super::*;

        const NATIVE_SMOKE_HELPER_ENV: &str = "FERRIC_TEST_PROCESS_SMOKE_HELPER";
        const NATIVE_SMOKE_MODE_ENV: &str = "FERRIC_TEST_PROCESS_SMOKE_MODE";
        const NATIVE_SMOKE_READY_ENV: &str = "FERRIC_TEST_PROCESS_SMOKE_READY";
        const NATIVE_SMOKE_INHERITED_FD_ENV: &str = "FERRIC_TEST_PROCESS_SMOKE_INHERITED_FD";
        const NATIVE_SMOKE_INHERITED_PORT_ENV: &str = "FERRIC_TEST_PROCESS_SMOKE_INHERITED_PORT";
        const NATIVE_SMOKE_TIMEOUT: Duration = Duration::from_secs(15);
        const F_GETFD: c_int = 1;
        const F_SETFD: c_int = 2;
        const FD_CLOEXEC: c_int = 1;
        const EBADF: i32 = 9;
        static NEXT_READY_MARKER: AtomicU64 = AtomicU64::new(0);

        unsafe extern "C" {
            fn fcntl(fd: c_int, command: c_int, ...) -> c_int;
        }

        struct ReadyMarker(PathBuf);

        impl ReadyMarker {
            fn new(mode: &str) -> Self {
                let sequence = NEXT_READY_MARKER.fetch_add(1, Ordering::Relaxed);
                Self(std::env::temp_dir().join(format!(
                    "ferric-process-smoke-{}-{sequence}-{mode}.ready",
                    std::process::id()
                )))
            }
        }

        impl Drop for ReadyMarker {
            fn drop(&mut self) {
                let _ = fs::remove_file(&self.0);
                let _ = fs::remove_file(staged_ready_path(&self.0));
            }
        }

        fn staged_ready_path(path: &Path) -> PathBuf {
            path.with_extension("ready.pending")
        }

        fn publish_ready(path: &Path, port: u16) {
            let pid = std::process::id();
            let staged = staged_ready_path(path);
            fs::write(&staged, format!("ferric-native-smoke-v1 {pid} {port}\n"))
                .expect("stage complete native smoke readiness coordinate");
            fs::rename(&staged, path).expect("atomically publish native smoke readiness");
        }

        fn parse_ready(value: &str, expected_pid: u32) -> Option<u16> {
            let mut fields = value.split_whitespace();
            let version = fields.next()?;
            let pid = fields.next()?.parse::<u32>().ok()?;
            let port = fields.next()?.parse::<u16>().ok()?;
            if version != "ferric-native-smoke-v1"
                || pid != expected_pid
                || port == 0
                || fields.next().is_some()
            {
                return None;
            }
            Some(port)
        }

        #[test]
        fn native_smoke_ready_marker_requires_complete_matching_coordinate() {
            assert_eq!(
                parse_ready("ferric-native-smoke-v1 42 8080\n", 42),
                Some(8080)
            );
            for invalid in [
                "",
                "ferric-native-smoke-v1",
                "ferric-native-smoke-v1 42",
                "ferric-native-smoke-v1 42 0",
                "ferric-native-smoke-v1 41 8080",
                "ferric-native-smoke-v1 42 8080 trailing",
                "ferric-native-smoke-v2 42 8080",
            ] {
                assert_eq!(parse_ready(invalid, 42), None, "accepted {invalid:?}");
            }
        }

        struct RetainedSmokeGuard {
            process: Option<LiveProcess>,
            child: Option<crate::test_process_containment::ContainedChild>,
            child_pid: u32,
            _ready: ReadyMarker,
        }

        impl RetainedSmokeGuard {
            fn process(&self) -> &LiveProcess {
                self.process
                    .as_ref()
                    .expect("smoke process remains retained")
            }

            fn child_pid(&self) -> u32 {
                self.child_pid
            }
        }

        impl Drop for RetainedSmokeGuard {
            fn drop(&mut self) {
                if let Some(process) = self.process.take() {
                    let cleanup = process.terminate().and_then(|_| {
                        if process.wait(Duration::from_secs(5))? {
                            Ok(())
                        } else {
                            Err(ProcessError::Operation(format!(
                                "retained Linux native-smoke process {} did not exit within 5s",
                                process.pid()
                            )))
                        }
                    });
                    if let Err(error) = cleanup {
                        crate::test_process_containment::abort_on_cleanup_failure(
                            "could not terminate retained Linux native-smoke process",
                            error,
                        );
                    }
                }
                if let Some(mut child) = self.child.take()
                    && let Err(error) = child.terminate_and_reap()
                {
                    crate::test_process_containment::abort_on_cleanup_failure(
                        "could not clean Linux native-smoke process tree",
                        error,
                    );
                }
            }
        }

        fn wait_for_ready(
            child: &mut crate::test_process_containment::ContainedChild,
            path: &Path,
        ) -> u16 {
            let deadline = Instant::now() + NATIVE_SMOKE_TIMEOUT;
            loop {
                if let Ok(value) = fs::read_to_string(path)
                    && let Some(port) = parse_ready(&value, child.child().id())
                {
                    return port;
                }
                if let Some(status) = child
                    .try_wait_leader()
                    .expect("inspect smoke helper status")
                {
                    panic!("native smoke helper exited before readiness: {status}");
                }
                assert!(
                    Instant::now() < deadline,
                    "native smoke helper did not publish readiness within {NATIVE_SMOKE_TIMEOUT:?}"
                );
                thread::sleep(Duration::from_millis(10));
            }
        }

        fn spawn_retained_smoke_child(
            mode: &str,
            inherited_listener: Option<(&TcpListener, u16)>,
        ) -> (RetainedSmokeGuard, u16) {
            crate::test_process_containment::ensure_current_process_tree_is_contained()
                .expect("install Linux native-smoke process containment");
            let ready = ReadyMarker::new(mode);
            let mut command =
                Command::new(std::env::current_exe().expect("current test executable"));
            command
                .args([
                    "--exact",
                    "server_process::platform::tests::native_process_smoke_helper",
                    "--nocapture",
                ])
                .env(NATIVE_SMOKE_HELPER_ENV, "1")
                .env(NATIVE_SMOKE_MODE_ENV, mode)
                .env(NATIVE_SMOKE_READY_ENV, &ready.0)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            if let Some((listener, port)) = inherited_listener {
                let inherited_fd = listener.as_raw_fd();
                let flags = unsafe { fcntl(inherited_fd, F_GETFD) };
                assert!(flags >= 0, "inspect inherited listener descriptor flags");
                command
                    .env(NATIVE_SMOKE_INHERITED_FD_ENV, inherited_fd.to_string())
                    .env(NATIVE_SMOKE_INHERITED_PORT_ENV, port.to_string());
                // Only the post-fork child clears CLOEXEC. This avoids a
                // process-wide inheritance race in the multithreaded harness.
                unsafe {
                    command.pre_exec(move || {
                        if fcntl(inherited_fd, F_SETFD, flags & !FD_CLOEXEC) < 0 {
                            Err(std::io::Error::last_os_error())
                        } else {
                            Ok(())
                        }
                    });
                }
            }

            let mut child = crate::test_process_containment::ContainedChild::spawn(&mut command)
                .expect("spawn contained native process smoke helper");
            let port = wait_for_ready(&mut child, &ready.0);
            let process = LiveProcess::acquire_child(child.child())
                .expect("retain the spawned child's exact pidfd");
            let child_pid = child.child().id();
            assert_eq!(process.pid(), child_pid);
            (
                RetainedSmokeGuard {
                    process: Some(process),
                    child: Some(child),
                    child_pid,
                    _ready: ready,
                },
                port,
            )
        }

        fn fixture_listener_state(pid: u32, port: u16, known_peer_pids: &[u32]) -> ListenerState {
            let mut relevant = RelevantListenerInodes::default();
            relevant.extend(
                listening_loopback_relevant_inodes(
                    &fs::read_to_string("/proc/net/tcp").expect("read native IPv4 table"),
                    port,
                    AddressFamily::V4,
                )
                .expect("parse native IPv4 table"),
            );
            if let Ok(table) = fs::read_to_string("/proc/net/tcp6") {
                relevant.extend(
                    listening_loopback_relevant_inodes(&table, port, AddressFamily::V6)
                        .expect("parse native IPv6 table"),
                );
            }
            listener_state_from_socket_inventories(
                pid,
                port,
                &relevant,
                socket_inodes_for_process(pid),
                known_peer_pids
                    .iter()
                    .copied()
                    .map(|peer_pid| (peer_pid, socket_inodes_for_process(peer_pid)))
                    .collect(),
            )
            .unwrap_or_else(ListenerState::Uninspectable)
        }

        fn wait_for_fixture_listener_state(
            process: &LiveProcess,
            port: u16,
            known_peer_pids: &[u32],
            expected: &ListenerState,
        ) -> ListenerState {
            let deadline = Instant::now() + NATIVE_SMOKE_TIMEOUT;
            loop {
                // These native sockets are created inside this test. Feeding
                // the production classifier the actual target inventory plus
                // the only fixture peers avoids making the smoke depend on
                // permission to enumerate unrelated root-owned `/proc/*/fd`
                // directories. An unaccounted socket inode still fails closed.
                let state = fixture_listener_state(process.pid(), port, known_peer_pids);
                if &state == expected {
                    return state;
                }
                assert!(
                    !process.wait(Duration::ZERO).expect("poll retained pidfd"),
                    "smoke helper exited while waiting for listener state {expected:?}; last state: {state:?}"
                );
                assert!(
                    Instant::now() < deadline,
                    "listener state did not become {expected:?}; last state: {state:?}"
                );
                thread::sleep(Duration::from_millis(10));
            }
        }

        /// A normal harness invocation returns immediately. The parent smoke
        /// explicitly re-runs only this test with one harmless socket mode.
        #[test]
        fn native_process_smoke_helper() {
            if std::env::var(NATIVE_SMOKE_HELPER_ENV).ok().as_deref() != Some("1") {
                return;
            }
            crate::test_process_containment::arm_current_process_parent_death_signal()
                .expect("arm native-smoke parent-death containment");
            let mode = std::env::var(NATIVE_SMOKE_MODE_ENV).expect("native smoke mode");
            let ready = PathBuf::from(
                std::env::var_os(NATIVE_SMOKE_READY_ENV).expect("native smoke ready path"),
            );
            let port = if mode == "inherited" {
                let inherited_fd = std::env::var(NATIVE_SMOKE_INHERITED_FD_ENV)
                    .expect("inherited listener fd")
                    .parse::<c_int>()
                    .expect("numeric inherited listener fd");
                assert!(
                    unsafe { fcntl(inherited_fd, F_GETFD) } >= 0,
                    "listener descriptor must survive exec"
                );
                std::env::var(NATIVE_SMOKE_INHERITED_PORT_ENV)
                    .expect("inherited listener port")
                    .parse::<u16>()
                    .expect("numeric inherited listener port")
            } else {
                let listener = match mode.as_str() {
                    "ipv4-loopback" => TcpListener::bind((Ipv4Addr::LOCALHOST, 0)),
                    "ipv4-wildcard" => TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)),
                    "ipv6-loopback" => TcpListener::bind((Ipv6Addr::LOCALHOST, 0)),
                    "ipv6-wildcard" => TcpListener::bind((Ipv6Addr::UNSPECIFIED, 0)),
                    other => panic!("unknown native smoke mode {other:?}"),
                }
                .expect("bind native smoke listener");
                let port = listener.local_addr().unwrap().port();
                publish_ready(&ready, port);
                thread::sleep(Duration::from_secs(60));
                return;
            };
            publish_ready(&ready, port);
            thread::sleep(Duration::from_secs(60));
        }

        fn complete_socket_inventory(inodes: &[&str]) -> ProcessSocketInventory {
            ProcessSocketInventory::Complete(
                inodes.iter().map(|inode| (*inode).to_string()).collect(),
            )
        }

        fn classified_listener_state(
            pid: u32,
            port: u16,
            relevant: &RelevantListenerInodes,
            target: ProcessSocketInventory,
            peers: Vec<(u32, ProcessSocketInventory)>,
        ) -> ListenerState {
            listener_state_from_socket_inventories(pid, port, relevant, target, peers)
                .unwrap_or_else(ListenerState::Uninspectable)
        }

        fn ipv6_wildcard_listener_fixture() -> RelevantListenerInodes {
            let table = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n\
                0: 00000000000000000000000001000000:1F90 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000 1000 0 55555 1 0000000000000000\n\
                1: 00000000000000000000000000000000:1F90 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000 1000 0 66666 1 0000000000000000\n";
            listening_loopback_relevant_inodes(table, 8080, AddressFamily::V6).unwrap()
        }

        fn assert_uninspectable_shared_listener_owner_is_not_exclusive() {
            let relevant = RelevantListenerInodes {
                loopback: HashSet::from(["shared".to_string()]),
                wildcard: HashSet::new(),
            };
            let state = classified_listener_state(
                10,
                8080,
                &relevant,
                complete_socket_inventory(&["shared"]),
                vec![(
                    20,
                    ProcessSocketInventory::Uninspectable(
                        "permission denied while reading inherited socket descriptors".to_string(),
                    ),
                )],
            );
            let ListenerState::Uninspectable(detail) = state else {
                panic!("uninspectable shared owner was misclassified as exclusive")
            };
            assert!(detail.contains("PID 20 is uninspectable"));
            assert!(!ListenerState::Uninspectable(detail).permits_teardown());
        }

        #[test]
        fn proc_stat_parser_handles_spaces_and_closing_parentheses_in_comm() {
            let stat =
                "42 (llama worker) name) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 987654 20";
            assert_eq!(parse_proc_start_ticks(stat), Some(987_654));
        }

        #[test]
        fn linux_token_binds_boot_and_process_start() {
            assert_eq!(
                format_linux_start_token("00000000-1111-2222-3333-444444444444", 987_654),
                "linux-boot-id:00000000-1111-2222-3333-444444444444;start-ticks:987654"
            );
        }

        #[test]
        fn linux_non_utf8_argv_is_uninspectable_not_lossy() {
            let error = parse_linux_argv(42, b"llama-server\0--model\0\xff\0").unwrap_err();
            let message = error.to_string();
            assert!(message.contains("argv element 2 is not UTF-8"));
            assert!(!message.contains('\u{fffd}'));

            let empty = parse_linux_argv(42, b"llama-server\0\0--port\08080\0").unwrap_err();
            assert!(empty.to_string().contains("argv element 1 is empty"));
        }

        #[test]
        fn loopback_listener_ownership_matrix() {
            let empty = RelevantListenerInodes::default();
            let state =
                classified_listener_state(10, 8080, &empty, complete_socket_inventory(&[]), vec![]);
            assert_eq!(state, ListenerState::Absent);
            assert!(state.permits_teardown());

            let loopback = RelevantListenerInodes {
                loopback: HashSet::from(["loopback".to_string()]),
                wildcard: HashSet::new(),
            };
            let state = classified_listener_state(
                10,
                8080,
                &loopback,
                complete_socket_inventory(&["loopback"]),
                vec![],
            );
            assert_eq!(state, ListenerState::OwnedByTarget);
            assert!(state.permits_teardown());

            let foreign = classified_listener_state(
                10,
                8080,
                &loopback,
                complete_socket_inventory(&[]),
                vec![(20, complete_socket_inventory(&["loopback"]))],
            );
            assert_eq!(foreign, ListenerState::OwnedByOther(vec![20]));
            assert!(!foreign.permits_teardown());

            let multiple_foreign = classified_listener_state(
                10,
                8080,
                &loopback,
                complete_socket_inventory(&[]),
                vec![
                    (20, complete_socket_inventory(&["loopback"])),
                    (30, complete_socket_inventory(&["loopback"])),
                ],
            );
            assert_eq!(multiple_foreign, ListenerState::OwnedByOther(vec![20, 30]));
            assert!(!multiple_foreign.permits_teardown());

            let shared = classified_listener_state(
                10,
                8080,
                &loopback,
                complete_socket_inventory(&["loopback"]),
                vec![(20, complete_socket_inventory(&["loopback"]))],
            );
            assert_eq!(shared, ListenerState::OwnedByOther(vec![10, 20]));
            assert!(!shared.permits_teardown());

            let wildcard = RelevantListenerInodes {
                loopback: HashSet::new(),
                wildcard: HashSet::from(["wildcard".to_string()]),
            };
            let wildcard_state = classified_listener_state(
                10,
                8080,
                &wildcard,
                complete_socket_inventory(&["wildcard"]),
                vec![],
            );
            assert_eq!(wildcard_state, ListenerState::OwnedByTargetWildcard);
            assert!(!wildcard_state.permits_teardown());

            let ipv6_wildcard = ipv6_wildcard_listener_fixture();
            assert_eq!(
                ipv6_wildcard,
                RelevantListenerInodes {
                    loopback: HashSet::new(),
                    wildcard: HashSet::from(["66666".to_string()]),
                }
            );
            let ipv6_wildcard_state = classified_listener_state(
                10,
                8080,
                &ipv6_wildcard,
                complete_socket_inventory(&["66666"]),
                vec![],
            );
            assert_eq!(ipv6_wildcard_state, ListenerState::OwnedByTargetWildcard);
            assert!(!ipv6_wildcard_state.permits_teardown());

            let dual_stack = classified_listener_state(
                10,
                8080,
                &ipv6_wildcard,
                complete_socket_inventory(&["66666"]),
                vec![(20, complete_socket_inventory(&["66666"]))],
            );
            assert_eq!(dual_stack, ListenerState::OwnedByOther(vec![10, 20]));
            assert!(!dual_stack.permits_teardown());

            let uninspectable = classified_listener_state(
                10,
                8080,
                &loopback,
                ProcessSocketInventory::Uninspectable("permission denied".to_string()),
                vec![],
            );
            assert!(matches!(uninspectable, ListenerState::Uninspectable(_)));
            assert!(!uninspectable.permits_teardown());

            assert_uninspectable_shared_listener_owner_is_not_exclusive();
        }

        #[test]
        fn linux_uninspectable_shared_listener_owner_is_not_exclusive() {
            assert_uninspectable_shared_listener_owner_is_not_exclusive();
        }

        #[test]
        fn tcp_parser_includes_loopback_and_wildcard_on_exact_listen_port() {
            let table = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n\
                0: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000 1000 0 12345 1 0000000000000000\n\
                1: 00000000:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000 1000 0 22222 1 0000000000000000\n\
                2: 0100007F:1F91 00000000:0000 0A 00000000:00000000 00:00000000 00000000 1000 0 33333 1 0000000000000000\n\
                3: 0100007F:1F90 00000000:0000 01 00000000:00000000 00:00000000 00000000 1000 0 44444 1 0000000000000000\n";
            assert_eq!(
                listening_loopback_relevant_inodes(table, 8080, AddressFamily::V4).unwrap(),
                RelevantListenerInodes {
                    loopback: HashSet::from(["12345".to_string()]),
                    wildcard: HashSet::from(["22222".to_string()]),
                }
            );
        }

        #[test]
        fn tcp6_parser_ignores_ipv6_loopback_but_keeps_wildcard() {
            assert_eq!(
                ipv6_wildcard_listener_fixture(),
                RelevantListenerInodes {
                    loopback: HashSet::new(),
                    wildcard: HashSet::from(["66666".to_string()]),
                }
            );
        }

        #[test]
        fn tcp_parser_rejects_a_listener_without_an_inspectable_inode() {
            let table = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n\
                0: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000 1000 0 0 1 0000000000000000\n";
            assert!(
                listening_loopback_relevant_inodes(table, 8080, AddressFamily::V4)
                    .unwrap_err()
                    .contains("no inspectable socket inode")
            );
        }

        #[test]
        fn native_listener_inventory_distinguishes_loopback_from_wildcard() {
            let loopback = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
            let loopback_port = loopback.local_addr().unwrap().port();
            let loopback_state = loopback_listener_state(std::process::id(), loopback_port);
            match &loopback_state {
                ListenerState::OwnedByTarget => assert!(loopback_state.permits_teardown()),
                ListenerState::Uninspectable(detail) => {
                    assert!(detail.contains("listener owner enumeration is incomplete"));
                    assert!(!loopback_state.permits_teardown());
                }
                other => panic!("unexpected native loopback state: {other:?}"),
            }

            let wildcard =
                std::net::TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, 0)).unwrap();
            let wildcard_port = wildcard.local_addr().unwrap().port();
            let wildcard_state = loopback_listener_state(std::process::id(), wildcard_port);
            match &wildcard_state {
                ListenerState::OwnedByTargetWildcard => {
                    assert!(!wildcard_state.permits_teardown())
                }
                ListenerState::Uninspectable(detail) => {
                    assert!(detail.contains("listener owner enumeration is incomplete"));
                    assert!(!wildcard_state.permits_teardown());
                }
                other => panic!("unexpected native wildcard state: {other:?}"),
            }
        }

        #[test]
        fn linux_pidfd_process_handle_smoke() {
            let (mut loopback, loopback_port) = spawn_retained_smoke_child("ipv4-loopback", None);
            let process = loopback.process();
            assert_eq!(
                wait_for_fixture_listener_state(
                    process,
                    loopback_port,
                    &[],
                    &ListenerState::OwnedByTarget,
                ),
                ListenerState::OwnedByTarget
            );
            let facts = process
                .inspect(loopback_port)
                .expect("inspect exact retained pidfd process");
            let pid = process.pid();
            let boot_id = fs::read_to_string("/proc/sys/kernel/random/boot_id")
                .expect("read native Linux boot ID")
                .trim()
                .to_string();
            let stat =
                fs::read_to_string(format!("/proc/{pid}/stat")).expect("read child process stat");
            let start_ticks =
                parse_proc_start_ticks(&stat).expect("parse child process start ticks");
            assert!(start_ticks > 0);
            assert_eq!(
                facts.identity.start_token,
                format_linux_start_token(&boot_id, start_ticks)
            );
            validate_start_token(&facts.identity.start_token)
                .expect("native boot-ID/start-ticks token must be canonical");
            assert_eq!(
                facts.identity.executable,
                fs::read_link(format!("/proc/{pid}/exe")).expect("read child executable identity")
            );
            let command_line =
                fs::read(format!("/proc/{pid}/cmdline")).expect("read child argv identity");
            assert_eq!(
                facts.identity.argv,
                parse_linux_argv(pid, &command_line).expect("parse child argv identity")
            );
            match facts.listener {
                ListenerState::OwnedByTarget => {}
                ListenerState::Uninspectable(ref detail) => {
                    assert!(detail.contains("listener owner enumeration is incomplete"));
                    assert!(!facts.listener.permits_teardown());
                }
                ref other => panic!("unexpected full-/proc listener state: {other:?}"),
            }
            assert!(
                !process
                    .wait(Duration::ZERO)
                    .expect("live pidfd must not poll as exited")
            );

            // Signal and poll only through the pidfd. The contained child is
            // consulted only after pidfd-observed exit, then its bounded
            // process-tree cleanup contract proves the helper was reaped.
            let retained = loopback.process.take().expect("retained loopback pidfd");
            let pidfd = retained.pidfd.as_raw_fd();
            assert!(
                unsafe { fcntl(pidfd, F_GETFD) } >= 0,
                "retained pidfd must be open before teardown"
            );
            assert!(
                retained
                    .terminate()
                    .expect("signal only through retained pidfd")
            );
            assert!(
                retained
                    .wait(Duration::from_secs(5))
                    .expect("poll only through retained pidfd")
            );
            let child = loopback
                .child
                .as_mut()
                .expect("contained smoke child remains retained");
            let status = child
                .try_wait_leader()
                .expect("inspect pidfd-terminated child")
                .expect("pidfd-observed exit must be visible through Child");
            assert!(
                !status.success(),
                "SIGKILLed helper must not report success"
            );
            child
                .terminate_and_reap()
                .expect("reap the exact-terminated Linux smoke process tree");
            drop(retained);
            assert_eq!(
                unsafe { fcntl(pidfd, F_GETFD) },
                -1,
                "dropping LiveProcess must close its pidfd"
            );
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(EBADF),
                "released pidfd must be rejected as bad"
            );

            let (wildcard, wildcard_port) = spawn_retained_smoke_child("ipv4-wildcard", None);
            let wildcard_state = wait_for_fixture_listener_state(
                wildcard.process(),
                wildcard_port,
                &[],
                &ListenerState::OwnedByTargetWildcard,
            );
            assert_eq!(wildcard_state, ListenerState::OwnedByTargetWildcard);
            assert!(!wildcard_state.permits_teardown());
            drop(wildcard);

            let (ipv6_wildcard, ipv6_wildcard_port) =
                spawn_retained_smoke_child("ipv6-wildcard", None);
            let ipv6_wildcard_state = wait_for_fixture_listener_state(
                ipv6_wildcard.process(),
                ipv6_wildcard_port,
                &[],
                &ListenerState::OwnedByTargetWildcard,
            );
            assert_eq!(ipv6_wildcard_state, ListenerState::OwnedByTargetWildcard);
            assert!(!ipv6_wildcard_state.permits_teardown());
            drop(ipv6_wildcard);

            let (ipv6_loopback, ipv6_loopback_port) =
                spawn_retained_smoke_child("ipv6-loopback", None);
            let ipv6_loopback_state = wait_for_fixture_listener_state(
                ipv6_loopback.process(),
                ipv6_loopback_port,
                &[],
                &ListenerState::Absent,
            );
            assert_eq!(
                ipv6_loopback_state,
                ListenerState::Absent,
                "exact ::1 cannot accept the managed IPv4 loopback endpoint"
            );
            drop(ipv6_loopback);

            // Clear CLOEXEC only in the forked child so one real loopback
            // socket inode is visibly owned by both this parent and the child.
            let inherited_listener =
                TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind inherited listener");
            let inherited_port = inherited_listener.local_addr().unwrap().port();
            let inherited_fd = inherited_listener.as_raw_fd();
            let (shared, ready_port) = spawn_retained_smoke_child(
                "inherited",
                Some((&inherited_listener, inherited_port)),
            );
            assert_eq!(ready_port, inherited_port);
            let parent_link = fs::read_link(format!("/proc/self/fd/{inherited_fd}"))
                .expect("read parent inherited socket link");
            let child_link =
                fs::read_link(format!("/proc/{}/fd/{inherited_fd}", shared.child_pid()))
                    .expect("read child inherited socket link");
            assert_eq!(child_link, parent_link);
            assert!(
                parent_link.to_string_lossy().starts_with("socket:["),
                "shared descriptor must visibly reference a socket inode"
            );
            let mut expected_owners = vec![std::process::id(), shared.child_pid()];
            expected_owners.sort_unstable();
            let expected_shared = ListenerState::OwnedByOther(expected_owners);
            let shared_state = wait_for_fixture_listener_state(
                shared.process(),
                inherited_port,
                &[std::process::id()],
                &expected_shared,
            );
            assert_eq!(shared_state, expected_shared);
            assert!(!shared_state.permits_teardown());
        }

        #[test]
        fn finite_poll_timeout_rounds_up_and_clamps() {
            assert_eq!(duration_to_poll_millis(Duration::from_nanos(1)), 1);
            assert_eq!(duration_to_poll_millis(Duration::ZERO), 0);
            assert_eq!(
                duration_to_poll_millis(Duration::from_secs(u64::MAX)),
                i32::MAX
            );
        }
    }
}

#[cfg(not(any(
    windows,
    all(
        target_os = "linux",
        target_endian = "little",
        target_pointer_width = "64",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
)))]
mod platform {
    use super::*;

    pub struct LiveProcess;

    impl LiveProcess {
        pub fn acquire_child(_child: &Child) -> Result<Self, ProcessError> {
            Err(ProcessError::Unsupported(std::env::consts::OS))
        }

        pub fn acquire(_pid: u32) -> Result<Self, ProcessError> {
            Err(ProcessError::Unsupported(std::env::consts::OS))
        }

        pub fn pid(&self) -> u32 {
            0
        }

        pub fn inspect(&self, _port: u16) -> Result<ProcessFacts, ProcessError> {
            Err(ProcessError::Unsupported(std::env::consts::OS))
        }

        pub fn terminate(&self) -> Result<bool, ProcessError> {
            Err(ProcessError::Unsupported(std::env::consts::OS))
        }

        pub fn wait(&self, _timeout: Duration) -> Result<bool, ProcessError> {
            Err(ProcessError::Unsupported(std::env::consts::OS))
        }
    }

    pub(super) fn loopback_listener_state(_pid: u32, _port: u16) -> ListenerState {
        ListenerState::Uninspectable(format!(
            "loopback listener ownership inspection is unsupported on {}",
            std::env::consts::OS
        ))
    }
}

pub use platform::LiveProcess;

impl RetainedProcess for LiveProcess {
    fn pid(&self) -> u32 {
        LiveProcess::pid(self)
    }

    fn inspect(&self, port: u16) -> Result<ProcessFacts, ProcessError> {
        LiveProcess::inspect(self, port)
    }

    fn terminate(&self) -> Result<bool, ProcessError> {
        LiveProcess::terminate(self)
    }

    fn wait(&self, timeout: Duration) -> Result<bool, ProcessError> {
        LiveProcess::wait(self, timeout)
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NativeProcessRuntime;

impl ProcessRuntime for NativeProcessRuntime {
    type Process = LiveProcess;

    fn acquire(&self, pid: u32) -> Result<Self::Process, ProcessError> {
        LiveProcess::acquire(pid)
    }
}

#[cfg(all(
    test,
    any(
        windows,
        all(
            target_os = "linux",
            target_endian = "little",
            target_pointer_width = "64",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    )
))]
mod retained_process_contract_tests {
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::rc::Rc;

    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Event {
        Acquire(u64),
        Inspect(u64),
        Terminate(u64),
        Wait(u64),
    }

    struct FakeRuntime {
        pid: u32,
        current_generation: Rc<Cell<u64>>,
        remap_on_inspect: Rc<Cell<Option<u64>>>,
        generations: Rc<HashMap<u64, ProcessFacts>>,
        events: Rc<RefCell<Vec<Event>>>,
    }

    struct FakeRetainedProcess {
        pid: u32,
        generation: u64,
        current_generation: Rc<Cell<u64>>,
        remap_on_inspect: Rc<Cell<Option<u64>>>,
        generations: Rc<HashMap<u64, ProcessFacts>>,
        events: Rc<RefCell<Vec<Event>>>,
    }

    impl ProcessRuntime for FakeRuntime {
        type Process = FakeRetainedProcess;

        fn acquire(&self, pid: u32) -> Result<Self::Process, ProcessError> {
            if pid != self.pid {
                return Err(ProcessError::NotFound(pid));
            }
            let generation = self.current_generation.get();
            self.events.borrow_mut().push(Event::Acquire(generation));
            Ok(FakeRetainedProcess {
                pid,
                generation,
                current_generation: Rc::clone(&self.current_generation),
                remap_on_inspect: Rc::clone(&self.remap_on_inspect),
                generations: Rc::clone(&self.generations),
                events: Rc::clone(&self.events),
            })
        }
    }

    impl RetainedProcess for FakeRetainedProcess {
        fn pid(&self) -> u32 {
            self.pid
        }

        fn inspect(&self, _port: u16) -> Result<ProcessFacts, ProcessError> {
            self.events
                .borrow_mut()
                .push(Event::Inspect(self.generation));
            if let Some(replacement) = self.remap_on_inspect.take() {
                self.current_generation.set(replacement);
            }
            self.generations
                .get(&self.generation)
                .cloned()
                .ok_or(ProcessError::NotFound(self.pid))
        }

        fn terminate(&self) -> Result<bool, ProcessError> {
            self.events
                .borrow_mut()
                .push(Event::Terminate(self.generation));
            Ok(true)
        }

        fn wait(&self, _timeout: Duration) -> Result<bool, ProcessError> {
            self.events.borrow_mut().push(Event::Wait(self.generation));
            Ok(true)
        }
    }

    fn facts(coordinate: u64) -> ProcessFacts {
        ProcessFacts {
            identity: ProcessIdentity {
                start_token: canonical_test_start_token(coordinate),
                executable: PathBuf::from(format!("/fixture/engine-{coordinate}")),
                argv: vec![
                    format!("engine-{coordinate}"),
                    "--port".to_string(),
                    "8080".to_string(),
                ],
            },
            listener: ListenerState::OwnedByTarget,
        }
    }

    #[test]
    fn retained_process_handle_identity_matrix() {
        let generation_one = facts(1);
        let generation_two = facts(2);
        let current_generation = Rc::new(Cell::new(1));
        let remap_on_inspect = Rc::new(Cell::new(Some(2)));
        let events = Rc::new(RefCell::new(Vec::new()));
        let runtime = FakeRuntime {
            pid: 77,
            current_generation: Rc::clone(&current_generation),
            remap_on_inspect: Rc::clone(&remap_on_inspect),
            generations: Rc::new(HashMap::from([
                (1, generation_one.clone()),
                (2, generation_two.clone()),
            ])),
            events: Rc::clone(&events),
        };

        // The numeric PID is remapped during inspection, after acquisition.
        // All later authority remains bound to generation one.
        let retained =
            acquire_matching_process(&runtime, 77, 8080, &generation_one.identity).unwrap();
        assert_eq!(current_generation.get(), 2);
        assert_eq!(retained.facts, generation_one);
        assert_eq!(retained.process.pid(), 77);
        retained.process.terminate().unwrap();
        retained.process.wait(Duration::from_secs(1)).unwrap();
        assert_eq!(
            events.borrow().as_slice(),
            [
                Event::Acquire(1),
                Event::Inspect(1),
                Event::Terminate(1),
                Event::Wait(1),
            ]
        );

        // PID reuse before acquisition observes the replacement generation
        // and fails without signaling or waiting on either generation.
        events.borrow_mut().clear();
        remap_on_inspect.set(None);
        let error = acquire_matching_process(&runtime, 77, 8080, &generation_one.identity)
            .err()
            .expect("replacement generation must not match");
        assert!(error.to_string().contains("start token"));
        assert_eq!(
            events.borrow().as_slice(),
            [Event::Acquire(2), Event::Inspect(2)]
        );

        // Each persisted identity coordinate independently fails closed.
        current_generation.set(1);
        for (coordinate, expected) in [
            ("start token", generation_two.identity.clone()),
            (
                "executable",
                ProcessIdentity {
                    start_token: generation_one.identity.start_token.clone(),
                    executable: PathBuf::from("/fixture/different-engine"),
                    argv: generation_one.identity.argv.clone(),
                },
            ),
            (
                "argv",
                ProcessIdentity {
                    start_token: generation_one.identity.start_token.clone(),
                    executable: generation_one.identity.executable.clone(),
                    argv: vec!["different-argv".to_string()],
                },
            ),
        ] {
            events.borrow_mut().clear();
            let error = acquire_matching_process(&runtime, 77, 8080, &expected)
                .err()
                .expect("identity mismatch must fail closed");
            assert!(error.to_string().contains(coordinate));
            assert_eq!(
                events.borrow().as_slice(),
                [Event::Acquire(1), Event::Inspect(1)]
            );
        }
    }

    #[test]
    fn pid_reuse_before_handle_acquisition_signals_nothing() {
        let generation_one = facts(1);
        let generation_two = facts(2);
        let events = Rc::new(RefCell::new(Vec::new()));
        let runtime = FakeRuntime {
            pid: 77,
            current_generation: Rc::new(Cell::new(2)),
            remap_on_inspect: Rc::new(Cell::new(None)),
            generations: Rc::new(HashMap::from([
                (1, generation_one.clone()),
                (2, generation_two),
            ])),
            events: Rc::clone(&events),
        };

        let error = acquire_matching_process(&runtime, 77, 8080, &generation_one.identity)
            .err()
            .expect("replacement generation must not match");

        assert!(error.to_string().contains("start token"));
        assert_eq!(
            events.borrow().as_slice(),
            [Event::Acquire(2), Event::Inspect(2)]
        );
    }

    #[test]
    fn pid_reuse_after_handle_acquisition_targets_original_handle() {
        let generation_one = facts(1);
        let generation_two = facts(2);
        let current_generation = Rc::new(Cell::new(1));
        let events = Rc::new(RefCell::new(Vec::new()));
        let runtime = FakeRuntime {
            pid: 77,
            current_generation: Rc::clone(&current_generation),
            remap_on_inspect: Rc::new(Cell::new(Some(2))),
            generations: Rc::new(HashMap::from([
                (1, generation_one.clone()),
                (2, generation_two),
            ])),
            events: Rc::clone(&events),
        };

        let retained =
            acquire_matching_process(&runtime, 77, 8080, &generation_one.identity).unwrap();
        assert_eq!(current_generation.get(), 2);
        retained.process.terminate().unwrap();
        retained.process.wait(Duration::from_secs(1)).unwrap();

        assert_eq!(
            events.borrow().as_slice(),
            [
                Event::Acquire(1),
                Event::Inspect(1),
                Event::Terminate(1),
                Event::Wait(1),
            ]
        );
    }
}

#[cfg(test)]
mod start_token_validation_tests {
    use super::*;

    #[cfg(any(
        windows,
        all(
            target_os = "linux",
            target_endian = "little",
            target_pointer_width = "64",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    ))]
    #[test]
    fn canonical_positive_u64_parser_rejects_noncanonical_values() {
        validate_start_token(&canonical_test_start_token(42)).unwrap();
        assert_eq!(parse_canonical_positive_u64("1"), Some(1));
        assert_eq!(
            parse_canonical_positive_u64("18446744073709551615"),
            Some(u64::MAX)
        );
        for invalid in [
            "",
            "0",
            "00",
            "01",
            "+1",
            "-1",
            " 1",
            "1 ",
            "1x",
            "18446744073709551616",
        ] {
            assert_eq!(
                parse_canonical_positive_u64(invalid),
                None,
                "accepted noncanonical integer {invalid:?}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_start_token_validation_is_exact() {
        validate_start_token("windows-filetime:1").unwrap();
        validate_start_token("windows-filetime:18446744073709551615").unwrap();

        for invalid in [
            "",
            "token",
            " windows-filetime:1",
            "windows-filetime:",
            "windows-filetime:0",
            "windows-filetime:01",
            "windows-filetime:+1",
            "windows-filetime:1 ",
            "windows-filetime:1;trailing",
            "windows-filetime:18446744073709551616",
            "linux-boot-id:00000000-1111-4222-8333-444444444444;start-ticks:1",
        ] {
            assert!(
                validate_start_token(invalid).is_err(),
                "accepted invalid Windows start token {invalid:?}"
            );
        }
    }

    #[cfg(all(
        target_os = "linux",
        target_endian = "little",
        target_pointer_width = "64",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    #[test]
    fn linux_start_token_validation_is_exact() {
        const BOOT_ID: &str = "00000000-1111-4222-8333-444444444444";
        validate_start_token(&format!("linux-boot-id:{BOOT_ID};start-ticks:1")).unwrap();
        validate_start_token(&format!(
            "linux-boot-id:{BOOT_ID};start-ticks:18446744073709551615"
        ))
        .unwrap();

        for invalid in [
            "",
            "token",
            " linux-boot-id:00000000-1111-4222-8333-444444444444;start-ticks:1",
            "windows-filetime:1",
            "linux-boot-id:00000000111142228333444444444444;start-ticks:1",
            "linux-boot-id:00000000-1111-4222-8333-44444444444;start-ticks:1",
            "linux-boot-id:00000000-1111-4222-8333-44444444444g;start-ticks:1",
            "linux-boot-id:00000000-1111-4222-8333-44444444444A;start-ticks:1",
            "linux-boot-id:00000000-1111-4222-8333-444444444444;start-ticks:",
            "linux-boot-id:00000000-1111-4222-8333-444444444444;start-ticks:0",
            "linux-boot-id:00000000-1111-4222-8333-444444444444;start-ticks:01",
            "linux-boot-id:00000000-1111-4222-8333-444444444444;start-ticks:+1",
            "linux-boot-id:00000000-1111-4222-8333-444444444444;start-ticks:1 ",
            "linux-boot-id:00000000-1111-4222-8333-444444444444;start-ticks:1;trailing",
            "linux-boot-id:00000000-1111-4222-8333-444444444444;start-ticks:18446744073709551616",
            "linux-boot-id:00000000-1111-4222-8333-444444444444;other:1;start-ticks:1",
        ] {
            assert!(
                validate_start_token(invalid).is_err(),
                "accepted invalid Linux start token {invalid:?}"
            );
        }
    }

    #[cfg(not(any(
        windows,
        all(
            target_os = "linux",
            target_endian = "little",
            target_pointer_width = "64",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    )))]
    #[test]
    fn unsupported_target_rejects_v2_start_token_authority() {
        assert!(validate_start_token("windows-filetime:1").is_err());
        assert!(
            validate_start_token(
                "linux-boot-id:00000000-1111-4222-8333-444444444444;start-ticks:1"
            )
            .is_err()
        );
    }
}
