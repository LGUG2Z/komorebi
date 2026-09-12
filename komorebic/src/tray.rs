use color_eyre::eyre::Result;
use color_eyre::eyre::WrapErr;
use color_eyre::eyre::bail;
use color_eyre::eyre::ensure;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use sysinfo::ProcessesToUpdate;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Foundation::ERROR_INVALID_PARAMETER;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::WAIT_OBJECT_0;
use windows::Win32::Foundation::WAIT_TIMEOUT;
use windows::Win32::Foundation::WPARAM;
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::CREATE_NO_WINDOW;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::System::Threading::OpenProcess;
use windows::Win32::System::Threading::PROCESS_NAME_WIN32;
use windows::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION;
use windows::Win32::System::Threading::PROCESS_SYNCHRONIZE;
use windows::Win32::System::Threading::PROCESS_TERMINATE;
use windows::Win32::System::Threading::QueryFullProcessImageNameW;
use windows::Win32::System::Threading::TerminateProcess;
use windows::Win32::System::Threading::WaitForSingleObject;
use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::Win32::UI::WindowsAndMessaging::WM_CLOSE;
use windows::core::PWSTR;
use windows::core::w;

const EXECUTABLE: &str = "komorebi-tray.exe";
const STOP_TIMEOUT_MS: u32 = 5000;

fn resolve_executable(
    cli_path: &Path,
    is_file: impl FnOnce(&Path) -> bool,
    on_path: impl FnOnce() -> Option<PathBuf>,
) -> Result<PathBuf> {
    let sibling = cli_path.with_file_name(EXECUTABLE);
    if is_file(&sibling) {
        return Ok(sibling);
    }
    if let Some(path) = on_path() {
        return Ok(path);
    }
    bail!(
        "could not find komorebi-tray.exe; install it beside komorebic.exe or on PATH before using --tray"
    )
}

pub fn prepare_start() -> Result<Option<PathBuf>> {
    if !process_ids()?.is_empty() {
        return Ok(None);
    }
    resolve_executable(&std::env::current_exe()?, Path::is_file, || {
        which::which(EXECUTABLE).ok()
    })
    .map(Some)
}

fn launch_if_needed(
    executable: Option<&Path>,
    running: bool,
    spawn: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    if !running && let Some(executable) = executable {
        spawn(executable)?;
    }
    Ok(())
}

pub fn start(executable: Option<PathBuf>) -> Result<()> {
    // Recheck after daemon startup. The tray's session-local mutex also handles
    // concurrent invocations that reach spawn at the same time.
    launch_if_needed(executable.as_deref(), !process_ids()?.is_empty(), |path| {
        Command::new(path)
            .creation_flags(CREATE_NO_WINDOW.0)
            .spawn()
            .wrap_err_with(|| format!("could not start {}", path.display()))?;
        println!("Started {}", path.display());
        Ok(())
    })
}

fn session_id(pid: u32) -> Result<u32> {
    let mut session = 0;
    unsafe { ProcessIdToSessionId(pid, &mut session)? };
    Ok(session)
}

fn is_session_tray(name: &str, session: Option<u32>, current_session: u32) -> bool {
    name.eq_ignore_ascii_case(EXECUTABLE) && session == Some(current_session)
}

fn process_ids() -> Result<Vec<u32>> {
    let session = session_id(unsafe { GetCurrentProcessId() })?;
    let mut system = sysinfo::System::new();
    system.refresh_processes(ProcessesToUpdate::All, true);
    Ok(system
        .processes()
        .iter()
        .filter_map(|(pid, process)| {
            let id = pid.as_u32();
            is_session_tray(
                &process.name().to_string_lossy(),
                session_id(id).ok(),
                session,
            )
            .then_some(id)
        })
        .collect())
}

trait ProcessControl {
    fn wait(&self, timeout_ms: u32) -> Result<bool>;
    fn request_close(&self) -> Result<()>;
    fn terminate(&self) -> Result<()>;
}

fn stop_process(process: &impl ProcessControl, force: bool) -> Result<()> {
    if process.wait(0)? {
        return Ok(());
    }
    if !force && process.request_close().is_ok() && process.wait(STOP_TIMEOUT_MS)? {
        return Ok(());
    }
    process.terminate()?;
    ensure!(
        process.wait(STOP_TIMEOUT_MS)?,
        "komorebi-tray did not exit after termination"
    );
    Ok(())
}

struct TrayProcess {
    handle: HANDLE,
    pid: u32,
}

impl TrayProcess {
    fn open(pid: u32) -> Result<Option<Self>> {
        let handle = match unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE,
                false,
                pid,
            )
        } {
            Ok(handle) => handle,
            Err(error) if error.code() == ERROR_INVALID_PARAMETER.to_hresult() => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let process = Self { handle, pid };
        if process.wait(0)? {
            return Ok(None);
        }
        // Verify identity after opening, then retain the handle throughout stop:
        // a recycled PID must never cause a different program to be terminated.
        let mut path = vec![0u16; 32768];
        let mut length = path.len() as u32;
        unsafe {
            QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                PWSTR(path.as_mut_ptr()),
                &mut length,
            )?
        };
        let path = PathBuf::from(OsString::from_wide(&path[..length as usize]));
        let current_session = session_id(unsafe { GetCurrentProcessId() })?;
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if !is_session_tray(&name, session_id(pid).ok(), current_session) {
            return Ok(None);
        }
        Ok(Some(process))
    }
}

impl ProcessControl for TrayProcess {
    fn wait(&self, timeout_ms: u32) -> Result<bool> {
        match unsafe { WaitForSingleObject(self.handle, timeout_ms) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(windows::core::Error::from_thread().into()),
        }
    }

    fn request_close(&self) -> Result<()> {
        unsafe {
            // This is the hidden top-level window used by komorebi-tray. The
            // owner check avoids posting to a newly started replacement instance.
            if let Ok(hwnd) = FindWindowW(w!("KomorebiTrayWindow"), None) {
                let mut pid = 0;
                GetWindowThreadProcessId(hwnd, Some(&mut pid));
                if pid == self.pid {
                    PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0))?;
                }
            }
        }
        Ok(())
    }

    fn terminate(&self) -> Result<()> {
        if let Err(error) = unsafe { TerminateProcess(self.handle, 1) }
            && !self.wait(0)?
        {
            return Err(error.into());
        }
        Ok(())
    }
}

impl Drop for TrayProcess {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

pub fn stop(force: bool) -> Result<()> {
    for pid in process_ids()? {
        if let Some(process) = TrayProcess::open(pid)? {
            stop_process(&process, force)?;
            println!("Stopped komorebi-tray.exe (PID {pid})");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    #[test]
    fn sibling_executable_has_priority_and_path_is_a_fallback() {
        let cli = Path::new(r"C:\Program Files\komorebi\bin\komorebic.exe");
        let sibling = cli.with_file_name(EXECUTABLE);
        assert_eq!(
            resolve_executable(
                cli,
                |p| p == sibling,
                || panic!("PATH should not be needed")
            )
            .unwrap(),
            sibling
        );
        let fallback = PathBuf::from(r"C:\Tools\komorebi-tray.exe");
        assert_eq!(
            resolve_executable(cli, |_| false, || Some(fallback.clone())).unwrap(),
            fallback
        );
        assert!(
            resolve_executable(cli, |_| false, || None)
                .unwrap_err()
                .to_string()
                .contains("--tray")
        );
    }

    #[test]
    fn repeated_start_skips_an_existing_tray() {
        let path = Path::new(r"C:\Program Files\komorebi\bin\komorebi-tray.exe");
        let launches = Cell::new(0);
        for running in [false, true, true] {
            launch_if_needed(Some(path), running, |actual| {
                assert_eq!(actual, path);
                launches.set(launches.get() + 1);
                Ok(())
            })
            .unwrap();
        }
        launch_if_needed(None, false, |_| panic!("no launch was planned")).unwrap();
        assert_eq!(launches.get(), 1);
    }

    #[test]
    fn only_exact_tray_names_in_the_current_session_match() {
        assert!(is_session_tray("Komorebi-Tray.EXE", Some(2), 2));
        assert!(!is_session_tray(EXECUTABLE, Some(1), 2));
        assert!(!is_session_tray(EXECUTABLE, None, 2));
        assert!(!is_session_tray("other-komorebi-tray.exe", Some(2), 2));
    }

    struct FakeProcess {
        waits: RefCell<VecDeque<bool>>,
        calls: RefCell<Vec<String>>,
    }
    impl FakeProcess {
        fn new(waits: &[bool]) -> Self {
            Self {
                waits: RefCell::new(waits.iter().copied().collect()),
                calls: RefCell::new(Vec::new()),
            }
        }
    }
    impl ProcessControl for FakeProcess {
        fn wait(&self, timeout: u32) -> Result<bool> {
            self.calls.borrow_mut().push(format!("wait:{timeout}"));
            Ok(self.waits.borrow_mut().pop_front().unwrap())
        }
        fn request_close(&self) -> Result<()> {
            self.calls.borrow_mut().push("close".to_owned());
            Ok(())
        }
        fn terminate(&self) -> Result<()> {
            self.calls.borrow_mut().push("terminate".to_owned());
            Ok(())
        }
    }

    #[test]
    fn graceful_stop_waits_for_cleanup_and_falls_back_only_on_timeout() {
        let responsive = FakeProcess::new(&[false, true]);
        stop_process(&responsive, false).unwrap();
        assert_eq!(*responsive.calls.borrow(), ["wait:0", "close", "wait:5000"]);
        let hung = FakeProcess::new(&[false, false, true]);
        stop_process(&hung, false).unwrap();
        assert_eq!(
            *hung.calls.borrow(),
            ["wait:0", "close", "wait:5000", "terminate", "wait:5000"]
        );
    }

    #[test]
    fn kill_is_immediate_and_an_exited_process_is_a_noop() {
        let running = FakeProcess::new(&[false, true]);
        stop_process(&running, true).unwrap();
        assert_eq!(
            *running.calls.borrow(),
            ["wait:0", "terminate", "wait:5000"]
        );
        let exited = FakeProcess::new(&[true]);
        stop_process(&exited, false).unwrap();
        assert_eq!(*exited.calls.borrow(), ["wait:0"]);
    }
}
