// Private SkyLight/CoreGraphics APIs
//
// Read operations (CGS*) are used for querying window server state.
// Write operations (SLS*) are used for animation screen update batching.
//
// What is only nice to have is not linked but looked up when it is first wanted (see
// `set_front_process`), so that a symbol that goes away does not stop komorebi from starting.
use std::ffi::CStr;
use std::ffi::c_char;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::OnceLock;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;

unsafe extern "C" {
    // Read operations - these APIs are undocumented but stable - their usage should not disrupt the window manager.
    pub fn CGSMainConnectionID() -> i32;
    pub fn CGSGetActiveSpace(cid: i32) -> u64;

    // Write operations for animation - these APIs are undocumented and I don't know how stable they are
    // SLSDisableUpdate freezes screen compositing - all window changes are batched
    pub fn SLSDisableUpdate(cid: i32) -> i32;
    // SLSReenableUpdate resumes compositing - all batched changes appear at once
    pub fn SLSReenableUpdate(cid: i32) -> i32;
}

#[cfg(test)]
thread_local! {
    /// How many [`BatchedUpdates`] there are on this thread, since there is no window server to
    /// ask under test
    static OPEN: std::cell::Cell<i32> = const { std::cell::Cell::new(0) };
}

/// Everything that changes on screen is held back until this is dropped, and then appears at once.
///
/// Moving a lot of windows one after another, which is what putting one workspace away and another
/// up is, is otherwise seen as it happens: the windows of both workspaces at the same time, with
/// the ones that take the longest to answer the last to go. Dropping it, which is also what an
/// early return does, is what lets the screen update again.
///
/// Keep it for what has to appear together and for no longer: nothing on screen changes while it
/// is held.
#[must_use = "updates are only held back for as long as this is kept"]
pub struct BatchedUpdates {
    connection_id: i32,
}

impl BatchedUpdates {
    pub fn begin() -> Self {
        // there is no window server to ask when testing
        let connection_id = if cfg!(test) {
            0
        } else {
            unsafe {
                let connection_id = CGSMainConnectionID();
                SLSDisableUpdate(connection_id);
                connection_id
            }
        };

        #[cfg(test)]
        OPEN.with(|open| open.set(open.get() + 1));

        Self { connection_id }
    }
}

impl Drop for BatchedUpdates {
    fn drop(&mut self) {
        if !cfg!(test) {
            unsafe { SLSReenableUpdate(self.connection_id) };
        }

        #[cfg(test)]
        OPEN.with(|open| open.set(open.get() - 1));
    }
}

unsafe extern "C" {
    // `RTLD_DEFAULT` is what is asked for here: everything the process has loaded
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;

/// Asks for a request to be treated as one that the user made: one from a process that is not in
/// the front is otherwise compared with the last input of the user, and turned away as expired if
/// there has been any since the request was stamped, which for komorebi is when it started.
const USER_GENERATED: u32 = 0x200;

/// How many requests in a row the window server may refuse before nothing is asked of it again
const MAX_CONSECUTIVE_FAILURES: u32 = 3;

#[repr(C)]
#[derive(Debug, Default)]
struct ProcessSerialNumber {
    high: u32,
    low: u32,
}

type GetProcessForPid = unsafe extern "C" fn(pid: i32, psn: *mut ProcessSerialNumber) -> i32;
type SetFrontProcessWithOptions =
    unsafe extern "C" fn(psn: *const ProcessSerialNumber, window_id: u32, options: u32) -> i32;

struct FrontProcessApi {
    get_process_for_pid: GetProcessForPid,
    set_front_process_with_options: SetFrontProcessWithOptions,
}

struct FrontProcess {
    api: Option<FrontProcessApi>,
    failures: AtomicU32,
}

fn lookup(name: &CStr) -> Option<NonNull<c_void>> {
    NonNull::new(unsafe { dlsym(RTLD_DEFAULT, name.as_ptr()) })
}

impl FrontProcess {
    fn resolve() -> Self {
        let get_process_for_pid = lookup(c"GetProcessForPID");
        let set_front_process_with_options = lookup(c"_SLPSSetFrontProcessWithOptions");

        let api = match (get_process_for_pid, set_front_process_with_options) {
            (Some(get_process_for_pid), Some(set_front_process_with_options)) => unsafe {
                Some(FrontProcessApi {
                    get_process_for_pid: std::mem::transmute::<*mut c_void, GetProcessForPid>(
                        get_process_for_pid.as_ptr(),
                    ),
                    set_front_process_with_options: std::mem::transmute::<
                        *mut c_void,
                        SetFrontProcessWithOptions,
                    >(
                        set_front_process_with_options.as_ptr()
                    ),
                })
            },
            _ => {
                tracing::warn!(
                    "the window server's way of bringing an application to the front was not found, activating applications the usual way instead"
                );
                None
            }
        };

        Self {
            api,
            failures: AtomicU32::new(0),
        }
    }

    fn request(&self, process_id: i32, window_id: u32) -> bool {
        let Some(api) = &self.api else {
            return false;
        };

        if self.failures.load(Ordering::Relaxed) >= MAX_CONSECUTIVE_FAILURES {
            return false;
        }

        // a process that is on its way out has no serial number, which is for the usual way to
        // make of what it will, and is nothing to hold against the window server
        let mut psn = ProcessSerialNumber::default();
        let status = unsafe { (api.get_process_for_pid)(process_id, &mut psn) };
        if status != 0 {
            tracing::debug!("no process serial number for process {process_id}: {status}");
            return false;
        }

        let status =
            unsafe { (api.set_front_process_with_options)(&psn, window_id, USER_GENERATED) };
        if status == 0 {
            self.failures.store(0, Ordering::Relaxed);
            return true;
        }

        let failures = self.failures.fetch_add(1, Ordering::Relaxed) + 1;
        if failures >= MAX_CONSECUTIVE_FAILURES {
            tracing::warn!(
                "the window server refused to bring process {process_id} to the front: {status}, and {failures} times in a row, so applications are activated the usual way from now on"
            );
        } else {
            tracing::warn!(
                "the window server refused to bring process {process_id} to the front: {status}"
            );
        }

        false
    }
}

/// Bring the application of a window to the front, the way a click on it would.
///
/// Activating an application from here, which is not the one in front, is a request that is
/// stamped with the time komorebi started, and the window server turns it away as expired once the
/// user has typed or clicked anything since: after that the application that was in front stays
/// there, and focus only moves in komorebi. This asks the window server directly, as a request
/// of the user's.
///
/// It is a private API that is looked up the first time it is wanted, and what it says is only
/// believed for as long as it keeps saying yes. This is for what is nice to have and not more:
/// `false` is for anything that did not work, whether the symbols are gone, the process is gone, or
/// the window server has refused three times in a row, and is for the caller to do what it always
/// did. It cannot tell a symbol that now takes other arguments from one that does what it did.
pub fn set_front_process(process_id: i32, window_id: u32) -> bool {
    // there is no window server to ask when testing
    if cfg!(test) {
        return false;
    }

    static FRONT_PROCESS: OnceLock<FrontProcess> = OnceLock::new();

    FRONT_PROCESS
        .get_or_init(FrontProcess::resolve)
        .request(process_id, window_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open() -> i32 {
        OPEN.with(std::cell::Cell::get)
    }

    #[test]
    fn updates_are_held_back_for_as_long_as_it_is_kept() {
        assert_eq!(open(), 0);

        let batched = BatchedUpdates::begin();
        assert_eq!(open(), 1);

        drop(batched);
        assert_eq!(open(), 0);
    }

    #[test]
    fn an_early_return_lets_the_screen_update_again() {
        fn fails() -> Result<(), ()> {
            let _batched = BatchedUpdates::begin();
            assert_eq!(open(), 1);

            Err(())?;

            Ok(())
        }

        assert!(fails().is_err());
        assert_eq!(open(), 0);
    }

    #[test]
    fn they_may_be_nested() {
        let outer = BatchedUpdates::begin();
        {
            let _inner = BatchedUpdates::begin();
            assert_eq!(open(), 2);
        }
        assert_eq!(open(), 1);

        drop(outer);
        assert_eq!(open(), 0);
    }

    extern "C" fn serial_number_found(pid: i32, psn: *mut ProcessSerialNumber) -> i32 {
        unsafe { (*psn).low = pid as u32 };
        0
    }

    extern "C" fn no_serial_number(_pid: i32, _psn: *mut ProcessSerialNumber) -> i32 {
        -600
    }

    fn front_process(
        get_process_for_pid: GetProcessForPid,
        set_front_process_with_options: SetFrontProcessWithOptions,
    ) -> FrontProcess {
        FrontProcess {
            api: Some(FrontProcessApi {
                get_process_for_pid,
                set_front_process_with_options,
            }),
            failures: AtomicU32::new(0),
        }
    }

    #[test]
    fn a_symbol_that_is_not_there_is_not_found() {
        assert!(lookup(c"komorebi_is_not_a_symbol_of_any_library").is_none());
    }

    // Not run with the others, because it is for finding out whether an update of macOS has taken
    // the symbols away, which komorebi is meant to carry on through: `cargo test -- --ignored`
    #[test]
    #[ignore = "needs the real symbols, to find out whether they are still there"]
    fn the_symbols_are_there() {
        let front_process = FrontProcess::resolve();
        assert!(front_process.api.is_some());

        // and the first of them is for this process, which it knows
        let api = front_process.api.unwrap();
        let mut psn = ProcessSerialNumber::default();
        let status = unsafe { (api.get_process_for_pid)(std::process::id() as i32, &mut psn) };
        assert_eq!(status, 0);
        assert!(psn.low != 0);
    }

    #[test]
    fn without_the_symbols_nothing_is_asked_and_nothing_goes_wrong() {
        let front_process = FrontProcess {
            api: None,
            failures: AtomicU32::new(0),
        };

        assert!(!front_process.request(1, 2));
    }

    #[test]
    fn nothing_is_asked_of_the_window_server_when_testing() {
        assert!(!set_front_process(1, 2));
    }

    #[test]
    fn a_request_that_is_accepted_is_made_for_the_user_and_for_the_window() {
        static CALLS: AtomicU32 = AtomicU32::new(0);

        extern "C" fn accepts(
            psn: *const ProcessSerialNumber,
            window_id: u32,
            options: u32,
        ) -> i32 {
            assert_eq!(unsafe { (*psn).low }, 79469);
            assert_eq!(window_id, 27576);
            assert_eq!(options, USER_GENERATED);
            CALLS.fetch_add(1, Ordering::Relaxed);
            0
        }

        let front_process = front_process(serial_number_found, accepts);

        assert!(front_process.request(79469, 27576));
        assert_eq!(CALLS.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn a_process_without_a_serial_number_is_left_to_the_usual_way_and_is_not_held_against_it() {
        static CALLS: AtomicU32 = AtomicU32::new(0);

        extern "C" fn counts(_: *const ProcessSerialNumber, _: u32, _: u32) -> i32 {
            CALLS.fetch_add(1, Ordering::Relaxed);
            0
        }

        let front_process = front_process(no_serial_number, counts);

        for _ in 0..MAX_CONSECUTIVE_FAILURES * 2 {
            assert!(!front_process.request(1, 2));
        }

        assert_eq!(CALLS.load(Ordering::Relaxed), 0);
        assert_eq!(front_process.failures.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn a_window_server_that_keeps_refusing_is_not_asked_again() {
        static CALLS: AtomicU32 = AtomicU32::new(0);

        extern "C" fn refuses(_: *const ProcessSerialNumber, _: u32, _: u32) -> i32 {
            CALLS.fetch_add(1, Ordering::Relaxed);
            1000
        }

        let front_process = front_process(serial_number_found, refuses);

        for _ in 0..MAX_CONSECUTIVE_FAILURES {
            assert!(!front_process.request(1, 2));
        }
        assert_eq!(CALLS.load(Ordering::Relaxed), MAX_CONSECUTIVE_FAILURES);

        for _ in 0..10 {
            assert!(!front_process.request(1, 2));
        }
        assert_eq!(CALLS.load(Ordering::Relaxed), MAX_CONSECUTIVE_FAILURES);
    }

    #[test]
    fn a_refusal_now_and_then_does_not_turn_it_off() {
        static CALLS: AtomicU32 = AtomicU32::new(0);

        // refuses every other request
        extern "C" fn refuses_every_other(_: *const ProcessSerialNumber, _: u32, _: u32) -> i32 {
            if CALLS.fetch_add(1, Ordering::Relaxed).is_multiple_of(2) {
                1000
            } else {
                0
            }
        }

        let front_process = front_process(serial_number_found, refuses_every_other);

        for round in 0..20 {
            assert_eq!(front_process.request(1, 2), round % 2 == 1);
        }

        assert_eq!(CALLS.load(Ordering::Relaxed), 20);
    }
}
