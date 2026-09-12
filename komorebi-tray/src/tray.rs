use crate::icon::Icon;
use crate::state::DisplayState;
use crate::state::IconKind;
use crate::state::tooltip_utf16;
use crate::subscription::Worker;
use color_eyre::eyre::Result;
use color_eyre::eyre::ensure;
use std::sync::Arc;
use std::sync::Mutex;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Foundation::ERROR_ALREADY_EXISTS;
use windows::Win32::Foundation::GetLastError;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::LRESULT;
use windows::Win32::Foundation::POINT;
use windows::Win32::Foundation::WPARAM;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2;
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::HiDpi::GetSystemMetricsForDpi;
use windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext;
use windows::Win32::UI::Shell::NIF_ICON;
use windows::Win32::UI::Shell::NIF_MESSAGE;
use windows::Win32::UI::Shell::NIF_SHOWTIP;
use windows::Win32::UI::Shell::NIF_TIP;
use windows::Win32::UI::Shell::NIM_ADD;
use windows::Win32::UI::Shell::NIM_DELETE;
use windows::Win32::UI::Shell::NIM_MODIFY;
use windows::Win32::UI::Shell::NIM_SETVERSION;
use windows::Win32::UI::Shell::NOTIFYICON_VERSION_4;
use windows::Win32::UI::Shell::NOTIFYICONDATAW;
use windows::Win32::UI::Shell::Shell_NotifyIconW;
use windows::Win32::UI::WindowsAndMessaging::AppendMenuW;
use windows::Win32::UI::WindowsAndMessaging::CREATESTRUCTW;
use windows::Win32::UI::WindowsAndMessaging::CreatePopupMenu;
use windows::Win32::UI::WindowsAndMessaging::CreateWindowExW;
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;
use windows::Win32::UI::WindowsAndMessaging::DestroyMenu;
use windows::Win32::UI::WindowsAndMessaging::DestroyWindow;
use windows::Win32::UI::WindowsAndMessaging::DispatchMessageW;
use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
use windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA;
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
use windows::Win32::UI::WindowsAndMessaging::GetMessageW;
use windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW;
use windows::Win32::UI::WindowsAndMessaging::HICON;
use windows::Win32::UI::WindowsAndMessaging::HMENU;
use windows::Win32::UI::WindowsAndMessaging::KillTimer;
use windows::Win32::UI::WindowsAndMessaging::MF_STRING;
use windows::Win32::UI::WindowsAndMessaging::MSG;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::Win32::UI::WindowsAndMessaging::PostQuitMessage;
use windows::Win32::UI::WindowsAndMessaging::RegisterClassW;
use windows::Win32::UI::WindowsAndMessaging::RegisterWindowMessageW;
use windows::Win32::UI::WindowsAndMessaging::SM_CXSMICON;
use windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;
use windows::Win32::UI::WindowsAndMessaging::SetTimer;
use windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW;
use windows::Win32::UI::WindowsAndMessaging::TPM_NONOTIFY;
use windows::Win32::UI::WindowsAndMessaging::TPM_RETURNCMD;
use windows::Win32::UI::WindowsAndMessaging::TPM_RIGHTBUTTON;
use windows::Win32::UI::WindowsAndMessaging::TrackPopupMenu;
use windows::Win32::UI::WindowsAndMessaging::TranslateMessage;
use windows::Win32::UI::WindowsAndMessaging::WM_APP;
use windows::Win32::UI::WindowsAndMessaging::WM_CLOSE;
use windows::Win32::UI::WindowsAndMessaging::WM_CONTEXTMENU;
use windows::Win32::UI::WindowsAndMessaging::WM_DESTROY;
use windows::Win32::UI::WindowsAndMessaging::WM_DISPLAYCHANGE;
use windows::Win32::UI::WindowsAndMessaging::WM_DPICHANGED;
use windows::Win32::UI::WindowsAndMessaging::WM_ENDSESSION;
use windows::Win32::UI::WindowsAndMessaging::WM_NCCREATE;
use windows::Win32::UI::WindowsAndMessaging::WM_NCDESTROY;
use windows::Win32::UI::WindowsAndMessaging::WM_NULL;
use windows::Win32::UI::WindowsAndMessaging::WM_SETTINGCHANGE;
use windows::Win32::UI::WindowsAndMessaging::WM_TIMER;
use windows::Win32::UI::WindowsAndMessaging::WNDCLASSW;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_NOACTIVATE;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_TOOLWINDOW;
use windows::Win32::UI::WindowsAndMessaging::WS_OVERLAPPED;
use windows::core::w;

const CLASS_NAME: windows::core::PCWSTR = w!("KomorebiTrayWindow");
const WM_TRAY: u32 = WM_APP + 1;
const WM_WORKSPACE: u32 = WM_APP + 2;
const EXIT_COMMAND: usize = 1;
const RETRY_TIMER: usize = 1;

type PendingState = Arc<Mutex<Option<DisplayState>>>;

struct App {
    hwnd: HWND,
    taskbar_created: u32,
    pending: PendingState,
    display: DisplayState,
    icon: Option<Icon>,
    icon_kind: Option<IconKind>,
    added: bool,
}

impl App {
    // Use HWND/uID identity: Windows binds a GUID icon to its original executable
    // path, which prevents an unsigned portable executable from being moved.
    fn icon_data(&self) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: 1,
            uFlags: NIF_ICON | NIF_TIP | NIF_MESSAGE | NIF_SHOWTIP,
            uCallbackMessage: WM_TRAY,
            hIcon: self
                .icon
                .as_ref()
                .map_or(HICON::default(), |icon| icon.handle),
            szTip: tooltip_utf16(&self.display.tooltip),
            ..Default::default()
        }
    }

    fn refresh(&mut self) -> Result<()> {
        let size = icon_size();
        let kind = self.display.icon;
        if self.icon.as_ref().is_none_or(|icon| icon.size != size) || self.icon_kind != Some(kind) {
            let icon = Icon::render(kind, size)?;
            self.icon = Some(icon);
            self.icon_kind = Some(kind);
        }
        let mut data = self.icon_data();
        unsafe {
            if self.added && Shell_NotifyIconW(NIM_MODIFY, &data).as_bool() {
                return Ok(());
            }
            self.added = Shell_NotifyIconW(NIM_ADD, &data).as_bool()
                || Shell_NotifyIconW(NIM_MODIFY, &data).as_bool();
            if self.added {
                data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
                if !Shell_NotifyIconW(NIM_SETVERSION, &data).as_bool() {
                    self.remove();
                }
            }
        }
        // Explorer may be absent at startup. The timer retries adding the icon.
        Ok(())
    }

    fn remove(&mut self) {
        if self.added {
            unsafe {
                let _ = Shell_NotifyIconW(NIM_DELETE, &self.icon_data());
            }
            self.added = false;
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.remove();
    }
}

fn icon_size() -> i32 {
    unsafe {
        let dpi = FindWindowW(w!("Shell_TrayWnd"), None)
            .map(|window| GetDpiForWindow(window))
            .unwrap_or_else(|_| GetDpiForSystem())
            .max(96);
        GetSystemMetricsForDpi(SM_CXSMICON, dpi).clamp(16, 256)
    }
}

pub fn run() -> Result<()> {
    // Keep the handle open for the entire process lifetime; ownership is unnecessary.
    let instance = unsafe { CreateMutexW(None, false, w!("Local\\KomorebiTray"))? };
    let already_running = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let _instance = Instance(instance);
    if already_running {
        return Ok(());
    }
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)?;
    }

    let pending = Arc::new(Mutex::new(None));
    let taskbar_created = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
    ensure!(
        taskbar_created != 0,
        "Could not register the taskbar restart message"
    );
    let mut app = Box::new(App {
        hwnd: HWND::default(),
        taskbar_created,
        pending: pending.clone(),
        display: DisplayState::disconnected(),
        icon: None,
        icon_kind: None,
        added: false,
    });
    let module = unsafe { GetModuleHandleW(None)? };
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: module.into(),
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    ensure!(
        unsafe { RegisterClassW(&class) } != 0,
        "Could not register the tray window"
    );
    // A hidden top-level window receives TaskbarCreated broadcasts; HWND_MESSAGE does not.
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            CLASS_NAME,
            w!("Komorebi Tray"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(module.into()),
            Some((&mut *app as *mut App).cast()),
        )?
    };
    let window = HiddenWindow(hwnd);
    app.hwnd = hwnd;
    app.refresh()?;
    ensure!(
        unsafe { SetTimer(Some(hwnd), RETRY_TIMER, 1000, None) } != 0,
        "Could not start the tray retry timer"
    );

    // Only the UI thread accesses App or owns HICONs. The worker posts an opaque wakeup,
    // while the mailbox coalesces rapid events without allocating messages on the heap.
    let window_address = hwnd.0 as isize;
    let worker = Worker::start(move |display| {
        let mut slot = pending.lock().unwrap_or_else(|error| error.into_inner());
        *slot = Some(display);
        drop(slot);
        unsafe {
            let _ = PostMessageW(
                Some(HWND(window_address as *mut _)),
                WM_WORKSPACE,
                WPARAM(0),
                LPARAM(0),
            );
        }
    })?;

    let result = message_loop();
    // Join before destroying the window, so the worker cannot post to a reused HWND.
    drop(worker);
    app.remove();
    drop(window);
    result
}

fn message_loop() -> Result<()> {
    let mut message = MSG::default();
    loop {
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
        if result == -1 {
            return Err(windows::core::Error::from_thread().into());
        }
        if result == 0 {
            return Ok(());
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // The Box<App> outlives this window, and only this message thread dereferences it.
    unsafe {
        if message == WM_NCCREATE {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        if message == WM_NCDESTROY {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }
        if message == WM_CLOSE
            || message == WM_DESTROY
            || (message == WM_ENDSESSION && wparam.0 != 0)
        {
            PostQuitMessage(0);
            return LRESULT(0);
        }
        if message == WM_TRAY {
            // Version 4 packs the event in LOWORD(lParam). No App borrow may span
            // TrackPopupMenu, which dispatches nested window messages.
            if (lparam.0 as u32 & 0xffff) == WM_CONTEXTMENU {
                let _ = show_menu(hwnd);
            }
            return LRESULT(0);
        }
        let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
        if !pointer.is_null() {
            let app = &mut *pointer;
            if message == WM_WORKSPACE {
                let display = app
                    .pending
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .take();
                if let Some(display) = display {
                    app.display = display;
                    let _ = app.refresh();
                }
                return LRESULT(0);
            }
            if message == app.taskbar_created {
                app.added = false;
                let _ = app.refresh();
                return LRESULT(0);
            }
            if message == WM_TIMER && wparam.0 == RETRY_TIMER {
                if !app.added
                    || app
                        .icon
                        .as_ref()
                        .is_none_or(|icon| icon.size != icon_size())
                    || app.icon_kind != Some(app.display.icon)
                {
                    let _ = app.refresh();
                }
                return LRESULT(0);
            }
            if message == WM_DPICHANGED
                || message == WM_DISPLAYCHANGE
                || message == WM_SETTINGCHANGE
            {
                let _ = app.refresh();
                return LRESULT(0);
            }
        }
        DefWindowProcW(hwnd, message, wparam, lparam)
    }
}

fn show_menu(hwnd: HWND) -> Result<()> {
    unsafe {
        let menu = Menu(CreatePopupMenu()?);
        AppendMenuW(menu.0, MF_STRING, EXIT_COMMAND, w!("Exit"))?;
        let mut position = POINT::default();
        GetCursorPos(&mut position)?;
        let _ = SetForegroundWindow(hwnd);
        let command = TrackPopupMenu(
            menu.0,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
            position.x,
            position.y,
            None,
            hwnd,
            None,
        );
        // Ensure clicking outside the menu dismisses it on subsequent invocations too.
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        if command.0 as usize == EXIT_COMMAND {
            PostQuitMessage(0);
        }
    }
    Ok(())
}

struct Instance(HANDLE);
impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

struct HiddenWindow(HWND);
impl Drop for HiddenWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.0), RETRY_TIMER);
            let _ = DestroyWindow(self.0);
        }
    }
}

struct Menu(HMENU);
impl Drop for Menu {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyMenu(self.0);
        }
    }
}
