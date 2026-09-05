//! Non-interactive Windows process scopes: no console windows are opened.
use crate::{CLEANUP_TIMEOUT, POLL_INTERVAL};
use std::io;
use std::process::{Command, ExitStatus};
use std::time::Instant;

fn with_cleanup_error(context: &str, error: io::Error, cleanup: io::Result<()>) -> io::Error {
    io::Error::other(format!("{context}: {error}; cleanup={cleanup:?}"))
}

pub(crate) struct ChildScope {
    child: Option<std::process::Child>,
    job: WindowsJob,
}

impl ChildScope {
    pub(crate) fn child(&self) -> &std::process::Child {
        self.child.as_ref().expect("owned Windows child")
    }
    pub(crate) fn child_mut(&mut self) -> &mut std::process::Child {
        self.child.as_mut().expect("owned Windows child")
    }
    pub(crate) fn terminate_leader(&mut self) -> io::Result<()> {
        self.child_mut().kill()
    }
    pub(crate) fn spawn(command: &mut Command) -> io::Result<Self> {
        Self::spawn_checked(command, |_, _| Ok(()))
    }

    fn spawn_checked(
        command: &mut Command,
        mut checkpoint: impl FnMut(SpawnStage, &std::process::Child) -> io::Result<()>,
    ) -> io::Result<Self> {
        use std::os::windows::{io::AsRawHandle, process::CommandExt};
        use windows_sys::Win32::System::{
            JobObjects::AssignProcessToJobObject,
            Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED},
        };

        // Create the kill-on-close Job first, then create the target suspended.
        // The unassigned guard owns the one failure window which command-group
        // 5.0.1 leaves open: if assignment fails after CreateProcess, it kills
        // and reaps that exact suspended child before returning.
        let job = WindowsJob::create()?;
        command.creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW);
        let mut unassigned = UnassignedWindowsChild::new(command.spawn()?);
        checkpoint(SpawnStage::BeforeAssign, unassigned.child_mut())?;
        let process_handle = unassigned
            .child_mut()
            .as_raw_handle()
            .cast::<core::ffi::c_void>();
        if unsafe { AssignProcessToJobObject(job.get(), process_handle) } == 0 {
            return Err(io::Error::new(
                io::Error::last_os_error().kind(),
                format!(
                    "cannot assign suspended child to kill-on-close Job: {}",
                    io::Error::last_os_error()
                ),
            ));
        }

        let mut tree = Self {
            child: Some(unassigned.take()),
            job,
        };
        checkpoint(SpawnStage::BeforeResume, tree.child())?;
        if let Err(error) = resume_windows_process(tree.child()) {
            let cleanup = tree.cleanup();
            return Err(with_cleanup_error(
                "cannot resume Job-owned suspended child",
                error,
                cleanup,
            ));
        }
        Ok(tree)
    }

    pub(crate) fn try_wait_leader(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child
            .as_mut()
            .expect("Windows process tree remains armed")
            .try_wait()
    }

    pub(crate) fn cleanup(&mut self) -> io::Result<()> {
        self.cleanup_checked(|| Ok(()))
    }

    // The checkpoint is a source-test seam, not a public process API. Production
    // cleanup does no work between the member snapshot and termination.
    pub(crate) fn cleanup_checked(
        &mut self,
        checkpoint: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<()> {
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;

        let Some(child) = self.child.as_mut() else {
            return Ok(());
        };
        let deadline = Instant::now()
            .checked_add(CLEANUP_TIMEOUT)
            .unwrap_or_else(Instant::now);
        // Job accounting can reach zero before the last process object's exit
        // event is signalled. Retain current member identities before initiating
        // termination, then wait for those exact objects as well as accounting.
        // TotalProcesses is cumulative: any admission after this fence makes
        // the retained snapshot incomplete, even if that new member disappears
        // from ActiveProcesses before we observe it. Refuse success in that
        // case; another snapshot or ActiveProcesses alone cannot close the race.
        let snapshot = self.job.accounting().and_then(|accounting| {
            self.job
                .retain_members(deadline)
                .map(|members| (accounting.TotalProcesses, members))
        });
        let checkpoint_error = snapshot.as_ref().ok().and_then(|_| checkpoint().err());
        let kill_error =
            (unsafe { TerminateJobObject(self.job.get(), 1) } == 0).then(io::Error::last_os_error);
        let (total_before_snapshot, retained) = match snapshot {
            Ok(snapshot) => snapshot,
            Err(error) => {
                eprintln!("cannot retain owned Job process identities: {error}");
                std::process::exit(125);
            }
        };
        if let Some(error) = checkpoint_error {
            eprintln!("Job cleanup checkpoint failed after retention: {error}");
            std::process::exit(125);
        }
        let mut leader_done = false;
        let mut reap_error = None;
        let mut membership_changed = false;

        loop {
            if !leader_done {
                match child.try_wait() {
                    Ok(Some(_)) => leader_done = true,
                    Ok(None) => {}
                    Err(error) => {
                        // ActiveProcesses reaching zero still proves that no
                        // executable remains even if std cannot read status.
                        leader_done = true;
                        reap_error = Some(error);
                    }
                }
            }

            let accounting = match self.job.accounting() {
                Ok(accounting) => accounting,
                Err(_) => std::process::exit(125),
            };
            membership_changed |= accounting.TotalProcesses != total_before_snapshot;
            let members_done = retained.iter().all(|process| process.exited());
            if leader_done && accounting.ActiveProcesses == 0 && members_done {
                // Reconcile after observing every retained exit event too, so
                // admission between the preceding counter read and those waits
                // cannot be mistaken for a fully covered snapshot.
                let final_accounting = match self.job.accounting() {
                    Ok(accounting) => accounting,
                    Err(_) => std::process::exit(125),
                };
                membership_changed |= final_accounting.TotalProcesses != total_before_snapshot;
                if final_accounting.ActiveProcesses == 0 {
                    if membership_changed {
                        eprintln!(
                            "Job membership changed after identity snapshot; cleanup cannot certify every native process exit"
                        );
                        std::process::exit(125);
                    }
                    break;
                }
            }
            if Instant::now() >= deadline {
                std::process::exit(125);
            }
            std::thread::sleep(POLL_INTERVAL);
        }

        // Windows has no zombie reaping obligation. Dropping the exact process
        // HANDLE is safe only after Job accounting proves ActiveProcesses == 0.
        self.child.take();
        if let Some(error) = kill_error {
            return Err(io::Error::new(
                error.kind(),
                format!("cannot terminate child Job Object: {error}"),
            ));
        }
        if let Some(error) = reap_error {
            return Err(io::Error::new(
                error.kind(),
                format!("cannot reap Job process-group leader: {error}"),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SpawnStage {
    BeforeAssign,
    BeforeResume,
}

impl Drop for ChildScope {
    fn drop(&mut self) {
        if self.child.is_some() && self.cleanup().is_err() {
            std::process::exit(125);
        }
    }
}

struct WindowsJob(usize);

impl WindowsJob {
    fn create() -> io::Result<Self> {
        use std::mem::size_of;
        use windows_sys::Win32::System::JobObjects::{
            CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };

        let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = Self(raw as usize);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = unsafe {
            SetInformationJobObject(
                job.get(),
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if configured == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }

    fn get(&self) -> windows_sys::Win32::Foundation::HANDLE {
        self.0 as windows_sys::Win32::Foundation::HANDLE
    }

    fn accounting(
        &self,
    ) -> io::Result<windows_sys::Win32::System::JobObjects::JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>
    {
        use std::mem::size_of;
        use windows_sys::Win32::System::JobObjects::{
            JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JobObjectBasicAccountingInformation,
            QueryInformationJobObject,
        };

        let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        let queried = unsafe {
            QueryInformationJobObject(
                self.get(),
                JobObjectBasicAccountingInformation,
                (&raw mut accounting).cast(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        };
        if queried == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(accounting)
    }

    fn retain_members(&self, deadline: Instant) -> io::Result<Vec<RetainedProcess>> {
        use windows_sys::Win32::Foundation::{ERROR_INVALID_PARAMETER, ERROR_MORE_DATA};
        use windows_sys::Win32::System::JobObjects::{
            IsProcessInJob, JOBOBJECT_BASIC_PROCESS_ID_LIST, JobObjectBasicProcessIdList,
            QueryInformationJobObject,
        };
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        };

        let mut capacity = self.accounting()?.ActiveProcesses.max(1) as usize;
        loop {
            let bytes = 8_usize
                .checked_add(
                    capacity
                        .checked_mul(std::mem::size_of::<usize>())
                        .ok_or_else(|| io::Error::other("Job member list overflow"))?,
                )
                .ok_or_else(|| io::Error::other("Job member list overflow"))?;
            let mut storage = vec![0_usize; bytes.div_ceil(std::mem::size_of::<usize>())];
            let info = storage
                .as_mut_ptr()
                .cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>();
            let size = u32::try_from(bytes)
                .map_err(|_| io::Error::other("Job member list exceeds native size"))?;
            if unsafe {
                QueryInformationJobObject(
                    self.get(),
                    JobObjectBasicProcessIdList,
                    info.cast(),
                    size,
                    std::ptr::null_mut(),
                )
            } == 0
            {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_MORE_DATA as i32) && Instant::now() < deadline
                {
                    capacity = capacity
                        .saturating_mul(2)
                        .max(unsafe { (*info).NumberOfAssignedProcesses } as usize);
                    continue;
                }
                return Err(error);
            }
            let count = unsafe { (*info).NumberOfProcessIdsInList } as usize;
            if count > capacity {
                return Err(io::Error::other("invalid Job member list length"));
            }
            let pids = unsafe { std::slice::from_raw_parts((*info).ProcessIdList.as_ptr(), count) };
            let mut processes = Vec::with_capacity(count);
            for pid in pids {
                let pid =
                    u32::try_from(*pid).map_err(|_| io::Error::other("invalid Job process ID"))?;
                let handle = unsafe {
                    OpenProcess(
                        PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                        0,
                        pid,
                    )
                };
                if handle.is_null() {
                    let error = io::Error::last_os_error();
                    // The process may have finished between the Job snapshot
                    // and handle acquisition. No PID-only termination follows.
                    if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
                        continue;
                    }
                    return Err(error);
                }
                let process = RetainedProcess(handle as usize);
                let mut belongs = 0;
                if unsafe { IsProcessInJob(process.get(), self.get(), &mut belongs) } == 0 {
                    return Err(io::Error::last_os_error());
                }
                if belongs != 0 {
                    processes.push(process);
                }
            }
            return Ok(processes);
        }
    }
}

struct RetainedProcess(usize);

impl RetainedProcess {
    fn get(&self) -> windows_sys::Win32::Foundation::HANDLE {
        self.0 as windows_sys::Win32::Foundation::HANDLE
    }
    fn exited(&self) -> bool {
        use windows_sys::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
        match unsafe { windows_sys::Win32::System::Threading::WaitForSingleObject(self.get(), 0) } {
            WAIT_OBJECT_0 => true,
            WAIT_TIMEOUT => false,
            _ => std::process::exit(125),
        }
    }
}

impl Drop for RetainedProcess {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.get());
        }
    }
}

impl Drop for WindowsJob {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.get());
        }
    }
}

struct UnassignedWindowsChild(Option<std::process::Child>);

impl UnassignedWindowsChild {
    fn new(child: std::process::Child) -> Self {
        Self(Some(child))
    }

    fn child_mut(&mut self) -> &mut std::process::Child {
        self.0.as_mut().expect("suspended child remains owned")
    }

    fn take(&mut self) -> std::process::Child {
        self.0.take().expect("suspended child remains owned")
    }
}

impl Drop for UnassignedWindowsChild {
    fn drop(&mut self) {
        let Some(mut child) = self.0.take() else {
            return;
        };
        if child.kill().is_err() {
            std::process::exit(125);
        }
        let deadline = Instant::now()
            .checked_add(CLEANUP_TIMEOUT)
            .unwrap_or_else(Instant::now);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if Instant::now() < deadline => std::thread::sleep(POLL_INTERVAL),
                Ok(None) | Err(_) => std::process::exit(125),
            }
        }
    }
}

fn resume_windows_process(child: &std::process::Child) -> io::Result<()> {
    use std::mem::size_of;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{
        Foundation::{INVALID_HANDLE_VALUE, WAIT_TIMEOUT},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First,
                Thread32Next,
            },
            Threading::{
                GetProcessIdOfThread, OpenThread, ResumeThread, THREAD_QUERY_LIMITED_INFORMATION,
                THREAD_SUSPEND_RESUME, WaitForSingleObject,
            },
        },
    };

    let process_id = child.id();

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let snapshot = WindowsJob(snapshot as usize);
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..THREADENTRY32::default()
    };
    let mut more = unsafe { Thread32First(snapshot.get(), &mut entry) };
    if more == 0 {
        return Err(io::Error::last_os_error());
    }

    let mut resumed = 0_u32;
    while more != 0 {
        if entry.th32OwnerProcessID == process_id {
            let thread = unsafe {
                OpenThread(
                    THREAD_SUSPEND_RESUME | THREAD_QUERY_LIMITED_INFORMATION,
                    0,
                    entry.th32ThreadID,
                )
            };
            if thread.is_null() {
                return Err(io::Error::last_os_error());
            }
            let thread = RetainedProcess(thread as usize);
            if unsafe { GetProcessIdOfThread(thread.get()) } != process_id
                || unsafe { WaitForSingleObject(child.as_raw_handle().cast(), 0) } != WAIT_TIMEOUT
            {
                return Err(io::Error::other(
                    "suspended child/thread identity changed before resume",
                ));
            }
            let previous = unsafe { ResumeThread(thread.get()) };
            if previous == u32::MAX {
                return Err(io::Error::last_os_error());
            }
            resumed = resumed.saturating_add(1);
        }
        more = unsafe { Thread32Next(snapshot.get(), &mut entry) };
    }
    if resumed == 0 {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "suspended child had no resumable thread",
        ));
    }
    Ok(())
}

mod parent {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::sync::OnceLock;

    use windows_sys::Win32::Foundation::{
        CloseHandle, FILETIME, HANDLE, INVALID_HANDLE_VALUE, WAIT_FAILED, WAIT_OBJECT_0,
        WAIT_TIMEOUT,
    };
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentProcessId, GetProcessTimes, OpenProcess,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };

    struct OwnedHandle(usize);

    impl OwnedHandle {
        fn new(handle: HANDLE) -> Self {
            Self(handle as usize)
        }

        fn get(&self) -> HANDLE {
            self.0 as HANDLE
        }
    }

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.get());
            }
        }
    }

    struct TestProcessTreeJob {
        _job: OwnedHandle,
    }

    impl TestProcessTreeJob {
        fn create() -> Result<Self, String> {
            let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if job.is_null() {
                return Err(format!(
                    "could not create Windows test-containment job: {}",
                    std::io::Error::last_os_error()
                ));
            }
            let job = OwnedHandle::new(job);

            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let configured = unsafe {
                SetInformationJobObject(
                    job.get(),
                    JobObjectExtendedLimitInformation,
                    (&raw const limits).cast::<c_void>(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if configured == 0 {
                return Err(format!(
                    "could not configure Windows test-containment job: {}",
                    std::io::Error::last_os_error()
                ));
            }

            if unsafe { AssignProcessToJobObject(job.get(), GetCurrentProcess()) } == 0 {
                return Err(format!(
                    "could not assign the Rust harness to its Windows containment job: {}",
                    std::io::Error::last_os_error()
                ));
            }

            let parent = open_exact_parent()?;
            let job_for_watcher = job.0;
            std::thread::Builder::new()
                .name("ferric-test-parent-watch".to_string())
                .spawn(move || {
                    let state = unsafe { WaitForSingleObject(parent.get(), u32::MAX) };
                    if state == WAIT_OBJECT_0 {
                        let terminated =
                            unsafe { TerminateJobObject(job_for_watcher as HANDLE, 0xEE) };
                        if terminated == 0 {
                            std::process::exit(125);
                        }
                        std::process::exit(125);
                    }
                    if state == WAIT_FAILED {
                        std::process::exit(125);
                    }
                    std::process::exit(125);
                })
                .map_err(|error| format!("could not start exact-parent watcher: {error}"))?;

            Ok(Self { _job: job })
        }
    }

    fn filetime_value(value: FILETIME) -> u64 {
        (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
    }

    fn process_creation_time(handle: HANDLE) -> Result<u64, String> {
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        if unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) } == 0
        {
            return Err(format!(
                "could not read Windows process generation: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(filetime_value(creation))
    }

    fn parent_pid() -> Result<u32, String> {
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(format!(
                "could not snapshot Windows processes: {}",
                std::io::Error::last_os_error()
            ));
        }
        let snapshot = OwnedHandle::new(snapshot);
        let current_pid = unsafe { GetCurrentProcessId() };
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..PROCESSENTRY32W::default()
        };
        let mut more = unsafe { Process32FirstW(snapshot.get(), &mut entry) };
        while more != 0 {
            if entry.th32ProcessID == current_pid {
                return (entry.th32ParentProcessID != 0)
                    .then_some(entry.th32ParentProcessID)
                    .ok_or_else(|| "Rust harness has no observable Windows parent".to_string());
            }
            more = unsafe { Process32NextW(snapshot.get(), &mut entry) };
        }
        Err(format!(
            "could not resolve Rust harness parent from Windows snapshot: {}",
            std::io::Error::last_os_error()
        ))
    }

    fn open_exact_parent() -> Result<OwnedHandle, String> {
        let pid = parent_pid()?;
        let handle = unsafe {
            OpenProcess(
                PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                pid,
            )
        };
        if handle.is_null() {
            return Err(format!(
                "could not open exact Windows parent {pid}: {}",
                std::io::Error::last_os_error()
            ));
        }
        let parent = OwnedHandle::new(handle);
        match unsafe { WaitForSingleObject(parent.get(), 0) } {
            WAIT_TIMEOUT => {}
            WAIT_OBJECT_0 => return Err("Windows parent exited during containment setup".into()),
            WAIT_FAILED => {
                return Err(format!(
                    "could not observe exact Windows parent: {}",
                    std::io::Error::last_os_error()
                ));
            }
            state => return Err(format!("unexpected Windows parent wait state {state}")),
        }
        let parent_created = process_creation_time(parent.get())?;
        let current_created = process_creation_time(unsafe { GetCurrentProcess() })?;
        if parent_created == 0 || parent_created > current_created {
            return Err(format!(
                "Windows parent PID {pid} changed generation during containment setup"
            ));
        }
        Ok(parent)
    }

    pub(super) fn watch_current_parent() -> Result<(), String> {
        static JOB: OnceLock<Result<TestProcessTreeJob, String>> = OnceLock::new();
        match JOB.get_or_init(TestProcessTreeJob::create) {
            Ok(_) => Ok(()),
            Err(error) => Err(error.clone()),
        }
    }
}

pub(crate) fn watch_current_parent() -> Result<(), String> {
    parent::watch_current_parent()
}

#[test]
fn windows_spawn_failure_rolls_back() {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };

    struct ExactHandle(HANDLE);
    impl Drop for ExactHandle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    watch_current_parent().expect("source harness containment");
    for fail_at in [SpawnStage::BeforeAssign, SpawnStage::BeforeResume] {
        let mut retained = None;
        let mut command =
            Command::new(std::env::current_exe().expect("Cargo-provided test harness"));
        // No fixture code can execute: both injected errors precede ResumeThread.
        command
            .arg("--list")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let started = Instant::now();
        let result = ChildScope::spawn_checked(&mut command, |stage, child| {
            if stage != fail_at {
                return Ok(());
            }
            let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, child.id()) };
            assert!(
                !handle.is_null(),
                "retain exact suspended child: {}",
                io::Error::last_os_error()
            );
            retained = Some(ExactHandle(handle));
            Err(io::Error::other(format!("injected {stage:?} failure")))
        });
        assert!(
            result.is_err(),
            "post-create rollback unexpectedly succeeded"
        );
        let retained = retained.expect("native child existed before injected failure");
        assert_eq!(
            unsafe { WaitForSingleObject(retained.0, 0) },
            WAIT_OBJECT_0,
            "rollback returned before exact suspended child exited"
        );
        assert!(started.elapsed() < CLEANUP_TIMEOUT + std::time::Duration::from_secs(1));
    }
}
