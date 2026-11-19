use crate::DATA_DIR;
use crate::DISPLAY_INDEX_PREFERENCES;
use crate::MONITOR_INDEX_PREFERENCES;
use crate::Rect;
use crate::WORKSPACE_MATCHING_RULES;
use crate::config_generation::WorkspaceMatchingRule;
use crate::reaper::HWNDS_CACHE;
use parking_lot::Mutex;
use parking_lot::MutexGuard;
use std::collections::HashMap;
use std::path::PathBuf;

static TEST_STATE: Mutex<()> = Mutex::new(());

/// Owns storage and configuration shared by stateful manager tests.
pub(crate) struct TestContext {
    _guard: MutexGuard<'static, ()>,
    workspace_rules: Vec<WorkspaceMatchingRule>,
    monitor_preferences: HashMap<usize, Rect>,
    display_preferences: HashMap<usize, String>,
    known_hwnds: HashMap<isize, (usize, usize)>,
}

impl TestContext {
    pub(crate) fn new() -> Self {
        let guard = TEST_STATE.lock();
        std::fs::create_dir_all(&*DATA_DIR).unwrap();
        Self {
            _guard: guard,
            workspace_rules: std::mem::take(&mut *WORKSPACE_MATCHING_RULES.lock()),
            monitor_preferences: std::mem::take(&mut *MONITOR_INDEX_PREFERENCES.lock()),
            display_preferences: std::mem::take(&mut *DISPLAY_INDEX_PREFERENCES.write()),
            known_hwnds: std::mem::take(&mut *HWNDS_CACHE.lock()),
        }
    }

    pub(crate) fn socket_path(&self) -> PathBuf {
        // Tests sharing this directory hold TEST_STATE, so one short name is sufficient.
        let path = DATA_DIR.join("wm.sock");
        assert!(
            path.to_string_lossy().len() < 108,
            "test socket path is too long: {path:?}"
        );
        path
    }
}

#[test]
fn test_context_restores_state_after_panic() {
    let context = TestContext::new();
    let expected_rules = context.workspace_rules.clone();
    let expected_monitors = context.monitor_preferences.clone();
    let expected_displays = context.display_preferences.clone();
    let expected_hwnds = context.known_hwnds.clone();
    assert!(WORKSPACE_MATCHING_RULES.lock().is_empty());
    assert!(MONITOR_INDEX_PREFERENCES.lock().is_empty());
    assert!(DISPLAY_INDEX_PREFERENCES.read().is_empty());
    assert!(HWNDS_CACHE.lock().is_empty());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _context = context;
        WORKSPACE_MATCHING_RULES.lock().push(WorkspaceMatchingRule {
            monitor_index: 9,
            workspace_index: 0,
            matching_rule: crate::config_generation::MatchingRule::Composite(vec![]),
            initial_only: false,
        });
        MONITOR_INDEX_PREFERENCES.lock().insert(9, Rect::default());
        DISPLAY_INDEX_PREFERENCES
            .write()
            .insert(9, "test".to_string());
        HWNDS_CACHE.lock().insert(123, (9, 0));
        std::fs::write(DATA_DIR.join("owned-test-file"), b"test").unwrap();
        panic!("exercise fixture cleanup");
    }));
    assert!(result.is_err());
    let _guard = TEST_STATE.lock();
    assert_eq!(*WORKSPACE_MATCHING_RULES.lock(), expected_rules);
    assert_eq!(*MONITOR_INDEX_PREFERENCES.lock(), expected_monitors);
    assert_eq!(*DISPLAY_INDEX_PREFERENCES.read(), expected_displays);
    assert_eq!(*HWNDS_CACHE.lock(), expected_hwnds);
    assert!(!DATA_DIR.exists());
}

impl Drop for TestContext {
    fn drop(&mut self) {
        *WORKSPACE_MATCHING_RULES.lock() = std::mem::take(&mut self.workspace_rules);
        *MONITOR_INDEX_PREFERENCES.lock() = std::mem::take(&mut self.monitor_preferences);
        *DISPLAY_INDEX_PREFERENCES.write() = std::mem::take(&mut self.display_preferences);
        *HWNDS_CACHE.lock() = std::mem::take(&mut self.known_hwnds);
        if let Err(error) = std::fs::remove_dir_all(&*DATA_DIR) {
            if std::thread::panicking() {
                tracing::error!("failed to remove test directory: {error}");
            } else {
                panic!("failed to remove test directory: {error}");
            }
        }
    }
}
