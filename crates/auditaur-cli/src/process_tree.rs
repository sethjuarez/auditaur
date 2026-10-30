//! A spawned app owned as a whole process tree, so stopping it leaves nothing behind.
//!
//! On Windows the root joins a Job Object that also kills the tree if Auditaur dies; on Unix it
//! leads its own process group. [`ProcessTree::stop`] ends every process in the tree and waits
//! until all are gone, so what they held (WebView2 data folders, ports, files) is free for the
//! next launch, even when the root already exited and left its children running.

use std::ops::{Deref, DerefMut};
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};

/// How long a stopped tree may take to exit.
pub const STOP_TIMEOUT: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(25);

pub struct ProcessTree {
    child: Child,
    #[cfg(windows)]
    job: job::Job,
}

impl ProcessTree {
    /// Spawn `command` as the root of a new tree, without a console window on Windows.
    ///
    /// On Windows the root starts suspended and resumes only once it is in the job, so nothing
    /// it starts can escape. Spawning fails rather than returning a tree it could not contain.
    pub fn spawn(command: &mut Command) -> std::io::Result<Self> {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_SUSPENDED: u32 = 0x0000_0004;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
            let mut child = command.spawn()?;
            match job::Job::adopt(&child).and_then(|job| job::resume(&child).map(|()| job)) {
                Ok(job) => Ok(Self { child, job }),
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    Err(std::io::Error::new(
                        error.kind(),
                        format!("could not contain the process tree in a job object: {error}"),
                    ))
                }
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
            Ok(Self {
                child: command.spawn()?,
            })
        }
    }

    /// Stop every process in the tree and wait, up to `timeout`, until all have exited.
    pub fn stop(&mut self, timeout: Duration) -> Result<String> {
        let pid = self.child.id();
        let deadline = Instant::now() + timeout;
        let root_exited = self.child.try_wait()?.is_some();
        let message = self.stop_tree(pid, root_exited, deadline)?;
        let _ = self.child.kill();
        let _ = self.child.wait();
        Ok(message)
    }

    #[cfg(windows)]
    fn stop_tree(&mut self, pid: u32, _root_exited: bool, deadline: Instant) -> Result<String> {
        let job = &self.job;
        if job.active_processes()? == 0 {
            return Ok(format!("process tree rooted at pid {pid} already exited"));
        }
        // A process leaves the job's active count as soon as it is terminated, but it finishes
        // tearing down (closing its files, locks, and WebView2 data folder) only when its handle
        // is signaled. Hold handles from before the kill and wait for those too. Best effort: a
        // process started between the snapshot and the kill is still covered by the count.
        let members = job.members().unwrap_or_default();
        job.terminate()?;
        loop {
            let left = job.active_processes()?;
            if left == 0 {
                break;
            }
            if Instant::now() >= deadline {
                return Err(anyhow!(
                    "{left} process(es) of the tree rooted at pid {pid} still running after it was stopped"
                ));
            }
            thread::sleep(POLL);
        }
        for member in &members {
            if !member.wait_until(deadline) {
                return Err(anyhow!(
                    "pid {} of the tree rooted at pid {pid} was still tearing down after it was stopped",
                    member.pid
                ));
            }
        }
        Ok(format!("stopped process tree rooted at pid {pid}"))
    }

    #[cfg(unix)]
    fn stop_tree(&mut self, pid: u32, _root_exited: bool, deadline: Instant) -> Result<String> {
        let group = pid as libc::pid_t;
        if !self.group_alive(group) {
            return Ok(format!("process tree rooted at pid {pid} already exited"));
        }
        // SAFETY: signals only the process group this tree created.
        unsafe { libc::kill(-group, libc::SIGTERM) };
        let grace = Instant::now() + (deadline - Instant::now()) / 2;
        if self.wait_for_group(group, grace) {
            return Ok(format!("stopped process tree rooted at pid {pid}"));
        }
        // SAFETY: as above.
        unsafe { libc::kill(-group, libc::SIGKILL) };
        if self.wait_for_group(group, deadline) {
            return Ok(format!(
                "killed process tree rooted at pid {pid} after it ignored SIGTERM"
            ));
        }
        Err(anyhow!("process group {pid} still running after SIGKILL"))
    }

    /// Whether any process of the group is left, reaping the root so it does not linger.
    #[cfg(unix)]
    fn group_alive(&mut self, group: libc::pid_t) -> bool {
        let _ = self.child.try_wait();
        // SAFETY: signal 0 only checks that the group exists.
        let result = unsafe { libc::kill(-group, 0) };
        result == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }

    #[cfg(unix)]
    fn wait_for_group(&mut self, group: libc::pid_t, until: Instant) -> bool {
        loop {
            if !self.group_alive(group) {
                return true;
            }
            if Instant::now() >= until {
                return false;
            }
            thread::sleep(POLL);
        }
    }
}

impl Deref for ProcessTree {
    type Target = Child;

    fn deref(&self) -> &Child {
        &self.child
    }
}

impl DerefMut for ProcessTree {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.child
    }
}

#[cfg(windows)]
mod job {
    use std::ffi::c_void;
    use std::io;
    use std::mem::{size_of, zeroed};
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
        JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
        TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    /// A Job Object that kills its processes when closed.
    pub struct Job(HANDLE);

    // SAFETY: a job handle may be used from any thread.
    unsafe impl Send for Job {}

    fn check(ok: i32) -> io::Result<()> {
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    impl Job {
        pub fn adopt(child: &Child) -> io::Result<Self> {
            // SAFETY: plain Win32 calls on handles owned here or by `child`.
            unsafe {
                let handle = CreateJobObjectW(null(), null());
                if handle.is_null() {
                    return Err(io::Error::last_os_error());
                }
                let job = Job(handle);
                let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
                limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                check(SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    &limits as *const _ as *const c_void,
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                ))?;
                check(AssignProcessToJobObject(
                    job.0,
                    child.as_raw_handle() as HANDLE,
                ))?;
                Ok(job)
            }
        }

        pub fn terminate(&self) -> io::Result<()> {
            // SAFETY: `self.0` is a live job handle.
            check(unsafe { TerminateJobObject(self.0, 1) })
        }

        pub fn active_processes(&self) -> io::Result<u32> {
            // SAFETY: the buffer is the struct the info class names.
            unsafe {
                let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = zeroed();
                check(QueryInformationJobObject(
                    self.0,
                    JobObjectBasicAccountingInformation,
                    &mut info as *mut _ as *mut c_void,
                    size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                    null_mut(),
                ))?;
                Ok(info.ActiveProcesses)
            }
        }

        /// Handles to the job's processes, to wait on their full exit. Best effort: a process
        /// that exits before it can be opened needs no waiting.
        pub fn members(&self) -> io::Result<Vec<Member>> {
            use windows_sys::Win32::System::JobObjects::JobObjectBasicProcessIdList;
            use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE};
            #[repr(C)]
            struct IdList {
                assigned: u32,
                listed: u32,
                ids: [usize; MAX_MEMBERS],
            }
            const MAX_MEMBERS: usize = 1024;
            let mut list = Box::new(IdList {
                assigned: 0,
                listed: 0,
                ids: [0; MAX_MEMBERS],
            });
            // SAFETY: the buffer has the JOBOBJECT_BASIC_PROCESS_ID_LIST layout and its true size.
            check(unsafe {
                QueryInformationJobObject(
                    self.0,
                    JobObjectBasicProcessIdList,
                    &mut *list as *mut IdList as *mut c_void,
                    size_of::<IdList>() as u32,
                    null_mut(),
                )
            })?;
            let listed = (list.listed as usize).min(MAX_MEMBERS);
            Ok(list.ids[..listed]
                .iter()
                .filter_map(|&id| {
                    let pid = id as u32;
                    // SAFETY: plain Win32 call; the handle is owned by the returned Member.
                    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
                    (!handle.is_null()).then_some(Member { pid, handle })
                })
                .collect())
        }
    }

    /// A process of the job, held open so its exit can be waited on.
    pub struct Member {
        pub pid: u32,
        handle: HANDLE,
    }

    impl Member {
        /// Wait until the process has fully exited, or `deadline` passes.
        pub fn wait_until(&self, deadline: std::time::Instant) -> bool {
            use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
            use windows_sys::Win32::System::Threading::WaitForSingleObject;
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            let ms = u32::try_from(left.as_millis()).unwrap_or(u32::MAX - 1);
            // SAFETY: `self.handle` is a live process handle.
            unsafe { WaitForSingleObject(self.handle, ms) == WAIT_OBJECT_0 }
        }
    }

    impl Drop for Member {
        fn drop(&mut self) {
            // SAFETY: opened in `members`, closed once.
            unsafe { CloseHandle(self.handle) };
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: closed once; kill-on-close ends anything still in the job.
            unsafe { CloseHandle(self.0) };
        }
    }

    /// Resume a child spawned with `CREATE_SUSPENDED` (its only thread is the main thread).
    pub fn resume(child: &Child) -> io::Result<()> {
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
        };
        use windows_sys::Win32::System::Threading::{
            OpenThread, ResumeThread, THREAD_SUSPEND_RESUME,
        };
        let pid = child.id();
        // SAFETY: plain Win32 calls; every handle opened here is closed here.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            let mut entry: THREADENTRY32 = zeroed();
            entry.dwSize = size_of::<THREADENTRY32>() as u32;
            let mut resumed = 0;
            let mut more = Thread32First(snapshot, &mut entry) != 0;
            let mut failure = None;
            while more {
                if entry.th32OwnerProcessID == pid {
                    let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                    if thread.is_null() || ResumeThread(thread) == u32::MAX {
                        failure = Some(io::Error::last_os_error());
                    } else {
                        resumed += 1;
                    }
                    if !thread.is_null() {
                        CloseHandle(thread);
                    }
                }
                more = Thread32Next(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
            match (resumed, failure) {
                (0, Some(error)) => Err(error),
                (0, None) => Err(io::Error::other("the suspended process has no threads")),
                _ => Ok(()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A root that starts a long-lived grandchild, writes its pid to `pid`, then waits for it
    /// (or, with `orphan`, exits at once and leaves it running).
    fn tree_command(dir: &std::path::Path, orphan: bool) -> Command {
        let pid_file = dir.join("pid");
        if cfg!(windows) {
            let wait = if orphan { "" } else { "; $p.WaitForExit()" };
            let script = format!(
                "$p = Start-Process -FilePath ping -ArgumentList '-n','120','127.0.0.1' -WindowStyle Hidden -PassThru; \
                 Set-Content -LiteralPath '{}' -Value $p.Id{wait}",
                pid_file.display()
            );
            let mut command = Command::new("powershell");
            command.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
            command
        } else {
            let wait = if orphan { "" } else { "; wait" };
            let script = format!("sleep 120 & echo $! > '{}'{wait}", pid_file.display());
            let mut command = Command::new("sh");
            command.args(["-c", &script]);
            command
        }
    }

    fn grandchild_pid(dir: &std::path::Path) -> u32 {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(pid) = fs::read_to_string(dir.join("pid"))
                .ok()
                .and_then(|text| text.trim().parse().ok())
            {
                return pid;
            }
            assert!(Instant::now() < deadline, "the grandchild never started");
            thread::sleep(POLL);
        }
    }

    /// Watches a pid for exit. On Windows it holds a handle opened while the process runs: a
    /// terminated process can stay listed (`tasklist`) while others still hold handles to it,
    /// but its handle is signaled the moment it exits, and the pid cannot be reused under us.
    struct Watch {
        pid: u32,
        #[cfg(windows)]
        handle: windows_sys::Win32::Foundation::HANDLE,
    }

    impl Watch {
        fn new(pid: u32) -> Self {
            #[cfg(windows)]
            {
                use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE};
                // SAFETY: plain Win32 call; the handle is closed on drop.
                let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
                assert!(!handle.is_null(), "pid {pid} is not running");
                Self { pid, handle }
            }
            #[cfg(unix)]
            Self { pid }
        }

        fn running(&self) -> bool {
            #[cfg(windows)]
            {
                use windows_sys::Win32::Foundation::WAIT_TIMEOUT;
                use windows_sys::Win32::System::Threading::WaitForSingleObject;
                // SAFETY: `self.handle` is a live process handle.
                unsafe { WaitForSingleObject(self.handle, 0) == WAIT_TIMEOUT }
            }
            #[cfg(unix)]
            Command::new("kill")
                .args(["-0", &self.pid.to_string()])
                .status()
                .unwrap()
                .success()
        }
    }

    #[cfg(windows)]
    impl Drop for Watch {
        fn drop(&mut self) {
            // SAFETY: opened in `new`, closed once.
            unsafe { windows_sys::Win32::Foundation::CloseHandle(self.handle) };
        }
    }

    fn stops_the_grandchild(orphan: bool) {
        let dir = tempfile::tempdir().unwrap();
        let mut tree = ProcessTree::spawn(&mut tree_command(dir.path(), orphan)).unwrap();
        let grandchild = Watch::new(grandchild_pid(dir.path()));
        assert!(grandchild.running());
        if orphan {
            let deadline = Instant::now() + Duration::from_secs(30);
            while tree.try_wait().unwrap().is_none() {
                assert!(Instant::now() < deadline, "the root never exited");
                thread::sleep(POLL);
            }
        }
        let message = tree.stop(STOP_TIMEOUT).unwrap();
        assert!(
            message.starts_with("stopped") || message.starts_with("killed"),
            "{message}"
        );
        // `stop` returns only once the whole tree is gone.
        assert!(
            !grandchild.running(),
            "pid {} survived: {message}",
            grandchild.pid
        );
    }

    #[test]
    fn stop_waits_for_grandchildren() {
        stops_the_grandchild(false);
    }

    #[test]
    fn stop_reaches_children_the_root_left_behind() {
        stops_the_grandchild(true);
    }

    #[test]
    fn stopping_an_exited_tree_is_quiet() {
        let mut command = if cfg!(windows) {
            let mut command = Command::new("cmd");
            command.args(["/c", "exit 0"]);
            command
        } else {
            let mut command = Command::new("sh");
            command.args(["-c", "exit 0"]);
            command
        };
        let mut tree = ProcessTree::spawn(&mut command).unwrap();
        let _ = tree.wait();
        // Windows may still be tearing down the hidden console's conhost, which `stop` then ends.
        let message = tree.stop(STOP_TIMEOUT).unwrap();
        assert!(
            message.contains("already exited") || message.starts_with("stopped"),
            "{message}"
        );
    }
}
