//! Findings from what Windows said about windows, as the recording's `--probe-windows` stored it.
//!
//! Each probed event gives [`invariants::check_probed`] a window's real state to compare with
//! komorebi's. One problem usually shows on several events in a row (a window komorebi has in front
//! and Windows has cloaked stays so until something changes it), so the same problem on the same
//! window across consecutive probes is one finding that says how long it lasted, as for the other
//! checks, not one per event.
//!
//! This module has no public items. It adds to the detectors of [`Analysis::findings`], which runs
//! it with the others, so the `overview` command and `brief` list its findings among theirs: it
//! is where komorebi for Windows' hidden-window violations come from (see
//! [`super::invariants`]), since a Windows state cannot say whether a window is hidden.
//!
//! # What is made of a probe
//!
//! Only an event that has both a probe (a `crate::sources::winprobe::WindowFacts`) and a state is
//! looked at, so a session recorded without `--probe-windows`, and any session of komorebi for
//! Mac, has no findings from here. Of those:
//!
//! - A probe read more than 1 s after its event is skipped: the window may have changed since.
//! - An event about one of the recorder's own windows (a program whose name starts with
//!   `komorebi-recorder`) is skipped, because komorebi does not manage them by design.
//! - The rest are judged by [`invariants::check_probed`]. A problem is followed for each window
//!   separately: it lasts from the first probe of that window that has it until the first later
//!   probe of the same window that does not. A problem still there at the window's last probe is
//!   marked [`Finding::persisted_to_end`].
//!
//! Each such run is one [`Finding`] of kind `invariant:<kind>`, with the violation's severity and a
//! summary of how many probes saw it and over how long. A problem seen at one probe only,
//! and gone at the window's next, is taken for a transition in progress, and its severity is
//! lowered one step (error to warn, anything else to info). The finding's `refs` names the first
//! event it was seen on and its `next` is the `trace` command for it.
//!
//! At most 20 `unmanaged-window` findings are reported for a recording: komorebi leaves toasts,
//! tooltips and dialogs alone on purpose, and a long recording sees very many.
//!
//! # Example
//!
//! Chrome is in front on the workspace that is showing, and after each of two events Windows says
//! it is cloaked, so it is one finding that lasted a second and was still so at the end:
//!
//! ```rust
//! use komorebi_recorder::analysis::Analysis;
//! use komorebi_recorder::analysis::flavor::Flavor;
//! use komorebi_recorder::sources::winprobe::WindowFacts;
//! use komorebi_recorder::testkit::SessionBuilder;
//! use komorebi_recorder::testkit::windows_state;
//! use komorebi_recorder::testkit::windows_window;
//! use serde_json::json;
//!
//! const CHROME: i64 = 131_072;
//!
//! let chrome = || windows_window(CHROME, "chrome.exe", Some([0, 0, 960, 1040]));
//! let state = || windows_state(65_537, vec![vec![chrome()]]);
//! let focus = json!({"type": "FocusChange", "content": ["SystemForeground", chrome()]});
//! let cloaked = WindowFacts {
//!     hwnd: CHROME as u32,
//!     at_ns: 0,
//!     exists: true,
//!     visible: true,
//!     cloaked: true,
//!     minimized: false,
//!     rect: Some([0, 0, 960, 1040]),
//!     komorebi_says: None,
//! };
//!
//! let (session, _) = SessionBuilder::new("doc-probed")
//!     .flavor(Flavor::Windows)
//!     .event(2.0, focus.clone(), Some(state()))
//!     .probe(cloaked.clone())
//!     .event(3.0, focus, Some(state()))
//!     .probe(cloaked)
//!     .build();
//!
//! let findings = Analysis::open(&session.dir)?.findings()?;
//! assert_eq!(findings.len(), 1);
//! assert_eq!(findings[0].kind, "invariant:visible-window-hidden");
//! assert!(findings[0].summary.contains("2 probe(s) over 1.0 s"));
//! assert!(findings[0].persisted_to_end);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! The other cases are covered by this module's unit tests, among them
//! `a_problem_that_is_gone_at_the_next_probe_was_a_transition_and_is_reported_as_one`,
//! `a_window_behind_the_front_one_that_windows_has_on_screen_is_leaking`,
//! `a_visible_window_komorebi_does_not_have_is_information_with_its_explanation` and
//! `our_own_windows_are_unmanaged_on_purpose_and_a_destroyed_one_is_nobodys_business`, and by
//! `tests/windows_flavor.rs::a_window_komorebi_shows_that_windows_cloaked_is_a_finding_in_the_overview`.

use super::Analysis;
use super::invariants;
use super::invariants::Severity;
use super::overview::Finding;
use std::collections::HashMap;

/// A probe read this long after its event may be of a window that has since changed.
pub(super) const STALE_NS: u64 = 1_000_000_000;

/// A problem seen once, for less than this, is usually a transition in progress.
const TRANSIENT_NS: u64 = 500_000_000;

/// The most windows komorebi does not manage that are reported: toasts, tooltips and dialogs are
/// rightly left alone, and a long recording sees very many.
const UNMANAGED_LIMIT: usize = 20;

struct Run {
    violation: invariants::Violation,
    first_seq: u64,
    first_ns: u64,
    last_ns: u64,
    probes: usize,
}

impl Analysis {
    pub(super) fn find_probed_problems(
        &self,
        found: &mut Vec<Finding>,
    ) -> color_eyre::eyre::Result<()> {
        let dir = self.session.dir.display().to_string();
        let mut open: HashMap<(String, u64), Run> = HashMap::new();
        let mut done: Vec<(Run, Option<u64>)> = Vec::new();

        for event in self.events() {
            let (Some(facts), Some(state_id)) = (&event.probe, event.state_id) else {
                continue;
            };
            if facts.at_ns.saturating_sub(event.recv_mach_ns) > STALE_NS {
                continue;
            }

            // komorebi does not manage this program's own windows, by design
            let exe = event.info.exe.as_deref();
            if exe.is_some_and(|exe| exe.to_ascii_lowercase().starts_with("komorebi-recorder")) {
                continue;
            }

            let snapshot = self.snapshot(state_id)?;
            let current = invariants::check_probed(&snapshot, facts, exe);

            // what was wrong about this window and no longer is
            let id = u64::from(facts.hwnd);
            let cleared: Vec<(String, u64)> = open
                .keys()
                .filter(|(kind, window)| {
                    *window == id && !current.iter().any(|violation| &violation.kind == kind)
                })
                .cloned()
                .collect();
            for key in cleared {
                if let Some(run) = open.remove(&key) {
                    done.push((run, Some(event.recv_mach_ns)));
                }
            }

            for violation in current {
                let key = (violation.kind.clone(), id);
                match open.get_mut(&key) {
                    Some(run) => {
                        run.last_ns = event.recv_mach_ns;
                        run.probes += 1;
                    }
                    None => {
                        open.insert(
                            key,
                            Run {
                                violation,
                                first_seq: event.seq,
                                first_ns: event.recv_mach_ns,
                                last_ns: event.recv_mach_ns,
                                probes: 1,
                            },
                        );
                    }
                }
            }
        }

        done.extend(open.into_values().map(|run| (run, None)));
        done.sort_by_key(|(run, _)| run.first_ns);

        let mut unmanaged = 0;
        for (run, cleared_at) in done {
            if run.violation.kind == "unmanaged-window" {
                unmanaged += 1;
                if unmanaged > UNMANAGED_LIMIT {
                    continue;
                }
            }

            let lasted = run.last_ns.saturating_sub(run.first_ns);
            let severity = if run.probes == 1 && lasted < TRANSIENT_NS && cleared_at.is_some() {
                match run.violation.severity {
                    Severity::Error => Severity::Warn,
                    _ => Severity::Info,
                }
            } else {
                run.violation.severity
            };

            let mut finding = self.finding(
                &format!("invariant:{}", run.violation.kind),
                severity,
                format!(
                    "{}; Windows said so on {} probe(s) over {:.1} s{}",
                    run.violation.message,
                    run.probes,
                    lasted as f64 / 1e9,
                    if cleared_at.is_none() {
                        ", and at the last one still"
                    } else {
                        ""
                    }
                ),
                run.first_ns,
                cleared_at.unwrap_or(run.last_ns),
            );
            finding.refs = vec![format!("event:{}", run.first_seq)];
            finding.window_ids = run.violation.window_ids.clone();
            finding.persisted_to_end = cleared_at.is_none();
            finding.next = Some(format!(
                "komorebi-recorder trace {dir} event:{}",
                run.first_seq
            ));
            found.push(finding);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::flavor::Flavor;
    use crate::sources::winprobe::WindowFacts;
    use crate::testkit::SessionBuilder;
    use crate::testkit::windows_state;
    use crate::testkit::windows_window;
    use serde_json::json;

    const FRONT: i64 = 131_072;
    const BEHIND: i64 = 262_144;

    fn facts(hwnd: i64, cloaked: bool, visible: bool) -> WindowFacts {
        WindowFacts {
            hwnd: u32::try_from(hwnd).expect("hwnd"),
            at_ns: 0,
            exists: true,
            visible,
            cloaked,
            minimized: false,
            rect: Some([0, 0, 960, 1040]),
            komorebi_says: None,
        }
    }

    fn focus(hwnd: i64, exe: &str) -> serde_json::Value {
        json!({"type": "FocusChange", "content": ["SystemForeground",
            windows_window(hwnd, exe, Some([0, 0, 960, 1040]))]})
    }

    /// One workspace showing a container with `FRONT` in front of `BEHIND`.
    fn state() -> serde_json::Value {
        let mut state = windows_state(
            65_537,
            vec![vec![
                windows_window(FRONT, "chrome.exe", Some([0, 0, 960, 1040])),
                windows_window(BEHIND, "code.exe", Some([0, 0, 960, 1040])),
            ]],
        );
        // the second tab is behind the first
        state["monitors"]["elements"][0]["workspaces"]["elements"][0]["containers"]["elements"]
            [0]["windows"]["focused"] = json!(0);
        state
    }

    fn findings(name: &str, build: impl FnOnce(SessionBuilder) -> SessionBuilder) -> Vec<Finding> {
        let (session, _) = build(SessionBuilder::new(name).flavor(Flavor::Windows)).build();
        let analysis = Analysis::open(&session.dir).expect("open");
        let mut found = Vec::new();
        analysis.find_probed_problems(&mut found).expect("findings");
        found
    }

    #[test]
    fn a_window_komorebi_shows_that_windows_has_cloaked_is_an_error_that_says_how_long() {
        let found = findings("probed-cloaked", |builder| {
            builder
                .event(2.0, focus(FRONT, "chrome.exe"), Some(state()))
                .probe(facts(FRONT, true, true))
                .event(3.0, focus(FRONT, "chrome.exe"), Some(state()))
                .probe(facts(FRONT, true, true))
        });

        assert_eq!(found.len(), 1, "{found:#?}");
        let finding = &found[0];
        assert_eq!(finding.kind, "invariant:visible-window-hidden");
        assert_eq!(finding.severity, Severity::Error);
        assert!(finding.summary.contains("cloaked"), "{}", finding.summary);
        assert!(
            finding.summary.contains("2 probe(s) over 1.0 s"),
            "{}",
            finding.summary
        );
        assert!(finding.persisted_to_end);
        assert_eq!(finding.window_ids, [FRONT as u64]);
    }

    #[test]
    fn a_problem_that_is_gone_at_the_next_probe_was_a_transition_and_is_reported_as_one() {
        let found = findings("probed-transient", |builder| {
            builder
                .event(2.0, focus(FRONT, "chrome.exe"), Some(state()))
                .probe(facts(FRONT, true, true))
                .event(2.2, focus(FRONT, "chrome.exe"), Some(state()))
                .probe(facts(FRONT, false, true))
        });

        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(
            found[0].severity,
            Severity::Warn,
            "downgraded from an error"
        );
        assert!(!found[0].persisted_to_end);
    }

    #[test]
    fn a_window_behind_the_front_one_that_windows_has_on_screen_is_leaking() {
        let found = findings("probed-leak", |builder| {
            builder
                .event(2.0, focus(BEHIND, "code.exe"), Some(state()))
                .probe(facts(BEHIND, false, true))
        });

        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].kind, "invariant:hidden-window-visible");
    }

    #[test]
    fn a_window_that_is_hidden_the_way_komorebi_hides_one_is_not_a_finding() {
        let found = findings("probed-fine", |builder| {
            builder
                // the window behind is cloaked, as komorebi hides it
                .event(2.0, focus(BEHIND, "code.exe"), Some(state()))
                .probe(facts(BEHIND, true, true))
                // and the one in front is shown
                .event(3.0, focus(FRONT, "chrome.exe"), Some(state()))
                .probe(facts(FRONT, false, true))
        });

        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn a_visible_window_komorebi_does_not_have_is_information_with_its_explanation() {
        let mut shown = facts(999, false, true);
        shown.komorebi_says =
            Some("not managed\n  matched ignore rule: exe == dialog.exe".to_string());
        let found = findings("probed-unmanaged", |builder| {
            builder
                .event(2.0, focus(999, "dialog.exe"), Some(state()))
                .probe(shown)
        });

        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].kind, "invariant:unmanaged-window");
        assert_eq!(found[0].severity, Severity::Info);
        assert!(
            found[0].summary.contains("komorebi says: not managed"),
            "{}",
            found[0].summary
        );
    }

    #[test]
    fn our_own_windows_are_unmanaged_on_purpose_and_a_destroyed_one_is_nobodys_business() {
        let mut gone = facts(999, false, false);
        gone.exists = false;
        let found = findings("probed-own", |builder| {
            builder
                .event(2.0, focus(777, "komorebi-recorder.exe"), Some(state()))
                .probe(facts(777, false, true))
                .event(3.0, focus(999, "dialog.exe"), Some(state()))
                .probe(gone)
        });

        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn a_session_without_probes_has_no_probed_findings_and_a_mac_one_is_never_judged_by_them() {
        let (session, _) = SessionBuilder::new("probed-none")
            .event(2.0, json!({"type": "Retile"}), None)
            .build();
        let analysis = Analysis::open(&session.dir).expect("open");
        let mut found = Vec::new();
        analysis.find_probed_problems(&mut found).expect("findings");
        assert!(found.is_empty());
    }
}
