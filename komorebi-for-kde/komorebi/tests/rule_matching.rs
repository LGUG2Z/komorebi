//! How a rule matches a window: the kinds (title, class, exe, path), every matching strategy,
//! composite rules, the last rule that matches winning, and a manage rule rescuing a window an
//! ignore rule covers. These are the pure functions every rule-driven behaviour of the daemon
//! goes through, so what the `rules`, `rules_class`, `rules_exe`, `manage_rules`, `title_rule`
//! and `startup_rules` end-to-end scenarios assert about WHICH window a rule catches is asserted
//! here with no window manager.
//!
//! Not covered here, and so still end-to-end: that the daemon applies the rule at the right moment
//! (when the window opens, not when it is renamed later), that the window then lands on the right
//! workspace and is hidden or shown, and what the OS reports as a window's title, class and
//! executable.

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::ApplicationIdentifier;
use komorebi::core::SocketMessage;
use komorebi::core::config_generation::IdWithIdentifier;
use komorebi::core::config_generation::MatchingRule;
use komorebi::core::config_generation::MatchingStrategy;
use komorebi::window::Window;
use komorebi::window::should_act;
use komorebi::window::should_act_individual;
use komorebi::window::should_act_window;
use komorebi::window_manager::WindowManager;
use komorebi::window_manager_event::KWinNotification;
use komorebi::window_manager_event::SystemNotification;
use komorebi::window_manager_event::WindowManagerEvent;
use regex::Regex;
use std::collections::HashMap;
use std::sync::Arc;

fn id(
    kind: ApplicationIdentifier,
    id: &str,
    strategy: Option<MatchingStrategy>,
) -> IdWithIdentifier {
    IdWithIdentifier {
        kind,
        id: id.to_string(),
        matching_strategy: strategy,
    }
}

fn simple(kind: ApplicationIdentifier, text: &str, strategy: MatchingStrategy) -> MatchingRule {
    MatchingRule::Simple(id(kind, text, Some(strategy)))
}

fn no_regex() -> HashMap<String, Regex> {
    HashMap::new()
}

/// The window every test asks about.
const TITLE: &str = "komorebi-e2e: rule target";
const EXE: &str = "kwrite";
const CLASS: &str = "org.kde.kwrite";
const PATH: &str = "/usr/bin/kwrite";

fn matches(rule: &IdWithIdentifier) -> bool {
    should_act_individual(TITLE, EXE, &[CLASS], PATH, rule, &no_regex())
}

fn kinds() -> [ApplicationIdentifier; 4] {
    [
        ApplicationIdentifier::Title,
        ApplicationIdentifier::Class,
        ApplicationIdentifier::Exe,
        ApplicationIdentifier::Path,
    ]
}

fn text_of(kind: ApplicationIdentifier) -> &'static str {
    match kind {
        ApplicationIdentifier::Title => TITLE,
        ApplicationIdentifier::Class => CLASS,
        ApplicationIdentifier::Exe => EXE,
        ApplicationIdentifier::Path => PATH,
    }
}

// mirrors: rules / a window an initial rule matches goes to the workspace the rule names (by title)
// mirrors: rules_class / a window a class rule matches goes to the workspace the rule names
// mirrors: rules_exe / a window an exe rule matches goes to the workspace the rule names
#[test]
fn equals_matches_the_whole_text_of_each_kind_and_nothing_else() {
    for kind in kinds() {
        let text = text_of(kind);
        assert!(
            matches(&id(kind, text, Some(MatchingStrategy::Equals))),
            "{kind:?}"
        );
        assert!(
            !matches(&id(
                kind,
                &text[..text.len() - 1],
                Some(MatchingStrategy::Equals)
            )),
            "{kind:?}: a prefix is not equal"
        );
        assert!(
            !matches(&id(
                kind,
                &format!("{text}x"),
                Some(MatchingStrategy::Equals)
            )),
            "{kind:?}: a longer text is not equal"
        );
    }
}

// mirrors: rules / a window of another title stays (the rule does not catch it)
// mirrors: rules_class / a window of another class stays on the first workspace
// mirrors: rules_exe / a window of another program stays on the first workspace
#[test]
fn each_kind_looks_at_its_own_part_of_the_window_only() {
    // the title of the window as the id of a class rule, and so on: no kind reads another's text
    for kind in kinds() {
        for other in kinds().into_iter().filter(|o| *o != kind) {
            assert!(
                !matches(&id(kind, text_of(other), Some(MatchingStrategy::Equals))),
                "a {kind:?} rule matched the {other:?} of the window"
            );
        }
    }
}

#[test]
fn starts_with_ends_with_and_contains_match_the_part_they_name() {
    for kind in kinds() {
        let text = text_of(kind);
        let head = &text[..4];
        let tail = &text[text.len() - 4..];
        let middle = &text[2..text.len() - 2];
        for (strategy, yes, no) in [
            (MatchingStrategy::StartsWith, head, tail),
            (MatchingStrategy::EndsWith, tail, head),
            (MatchingStrategy::Contains, middle, "not-in-the-window"),
        ] {
            assert!(
                matches(&id(kind, yes, Some(strategy.clone()))),
                "{kind:?} {strategy}"
            );
            assert!(
                !matches(&id(kind, no, Some(strategy.clone()))),
                "{kind:?} {strategy}"
            );
        }
    }
}

#[test]
fn every_negated_strategy_is_the_opposite_of_its_positive_one() {
    for kind in kinds() {
        let text = text_of(kind);
        let head = &text[..4];
        let tail = &text[text.len() - 4..];
        for (positive, negative, text) in [
            (
                MatchingStrategy::Equals,
                MatchingStrategy::DoesNotEqual,
                text,
            ),
            (
                MatchingStrategy::StartsWith,
                MatchingStrategy::DoesNotStartWith,
                head,
            ),
            (
                MatchingStrategy::EndsWith,
                MatchingStrategy::DoesNotEndWith,
                tail,
            ),
            (
                MatchingStrategy::Contains,
                MatchingStrategy::DoesNotContain,
                head,
            ),
        ] {
            for probe in [text, "zzzz-not-there"] {
                let p = matches(&id(kind, probe, Some(positive.clone())));
                let n = matches(&id(kind, probe, Some(negative.clone())));
                assert_ne!(p, n, "{kind:?}: {positive} and {negative} for {probe:?}");
            }
        }
    }
}

// mirrors: rules / a window a rule by the name of a workspace matches goes to that workspace
#[test]
fn a_regex_rule_matches_with_the_compiled_regex_and_nothing_without_one() {
    let pattern = "^komorebi-e2e: .* target$";
    let rule = id(
        ApplicationIdentifier::Title,
        pattern,
        Some(MatchingStrategy::Regex),
    );
    let mut compiled = HashMap::new();
    compiled.insert(pattern.to_string(), Regex::new(pattern).unwrap());

    assert!(should_act_individual(
        TITLE,
        EXE,
        &[CLASS],
        PATH,
        &rule,
        &compiled
    ));
    assert!(!should_act_individual(
        "another",
        EXE,
        &[CLASS],
        PATH,
        &rule,
        &compiled
    ));
    // the daemon compiles every regex when it loads the rules: an id it has not compiled matches nothing
    assert!(!should_act_individual(
        TITLE,
        EXE,
        &[CLASS],
        PATH,
        &rule,
        &no_regex()
    ));
}

#[test]
fn a_regex_rule_can_catch_any_of_the_classes_of_a_window() {
    let pattern = "^org\\.kde\\.";
    let rule = id(
        ApplicationIdentifier::Class,
        pattern,
        Some(MatchingStrategy::Regex),
    );
    let mut compiled = HashMap::new();
    compiled.insert(pattern.to_string(), Regex::new(pattern).unwrap());
    assert!(should_act_individual(
        TITLE,
        EXE,
        &["x", CLASS],
        PATH,
        &rule,
        &compiled
    ));
    assert!(!should_act_individual(
        TITLE,
        EXE,
        &["x", "y"],
        PATH,
        &rule,
        &compiled
    ));
}

// mirrors: rules_exe / a window an exe rule matches goes to the workspace the rule names
// (a rule written the Windows way, `kwrite.exe`, names the same program)
#[test]
fn an_exe_suffix_in_a_rule_id_is_ignored() {
    assert!(matches(&id(
        ApplicationIdentifier::Exe,
        "kwrite.exe",
        Some(MatchingStrategy::Equals)
    )));
    assert!(matches(&id(ApplicationIdentifier::Exe, "kwrite.exe", None)));
}

#[test]
fn a_rule_without_a_strategy_or_with_legacy_is_the_old_one() {
    for strategy in [None, Some(MatchingStrategy::Legacy)] {
        // title and class: starts or ends with the id
        assert!(matches(&id(
            ApplicationIdentifier::Title,
            "komorebi-e2e",
            strategy.clone()
        )));
        assert!(matches(&id(
            ApplicationIdentifier::Title,
            "rule target",
            strategy.clone()
        )));
        assert!(!matches(&id(
            ApplicationIdentifier::Title,
            "e2e: rule",
            strategy.clone()
        )));
        assert!(matches(&id(
            ApplicationIdentifier::Class,
            "org.kde",
            strategy.clone()
        )));
        assert!(matches(&id(
            ApplicationIdentifier::Class,
            "kwrite",
            strategy.clone()
        )));
        assert!(!matches(&id(
            ApplicationIdentifier::Class,
            "kde.kw",
            strategy.clone()
        )));
        // exe and path: equal to the id
        assert!(matches(&id(
            ApplicationIdentifier::Exe,
            EXE,
            strategy.clone()
        )));
        assert!(!matches(&id(
            ApplicationIdentifier::Exe,
            "kwri",
            strategy.clone()
        )));
        assert!(matches(&id(
            ApplicationIdentifier::Path,
            PATH,
            strategy.clone()
        )));
        assert!(!matches(&id(
            ApplicationIdentifier::Path,
            "/usr/bin",
            strategy.clone()
        )));
    }
}

// mirrors: rules / a window an ignore rule matches is not managed (which window)
#[test]
fn a_simple_rule_is_matched_by_one_condition_and_a_composite_by_all_of_them() {
    let title = id(
        ApplicationIdentifier::Title,
        TITLE,
        Some(MatchingStrategy::Equals),
    );
    let class = id(
        ApplicationIdentifier::Class,
        CLASS,
        Some(MatchingStrategy::Equals),
    );
    let other = id(
        ApplicationIdentifier::Exe,
        "not-kwrite",
        Some(MatchingStrategy::Equals),
    );

    let both = MatchingRule::Composite(vec![title.clone(), class.clone()]);
    let one_wrong = MatchingRule::Composite(vec![title.clone(), other.clone()]);
    let regex = no_regex();

    assert_eq!(
        should_act(
            TITLE,
            EXE,
            &[CLASS],
            PATH,
            std::slice::from_ref(&both),
            &regex
        ),
        Some(both.clone())
    );
    assert_eq!(
        should_act(TITLE, EXE, &[CLASS], PATH, &[one_wrong], &regex),
        None
    );
    assert_eq!(
        should_act(
            TITLE,
            EXE,
            &[CLASS],
            PATH,
            &[MatchingRule::Simple(title.clone())],
            &regex
        ),
        Some(MatchingRule::Simple(title))
    );
}

// mirrors: rules / a rule that is not initial sends the window back at the next event (which
// of two rules wins)
#[test]
fn of_two_rules_that_match_the_last_one_is_the_answer() {
    let first = simple(
        ApplicationIdentifier::Class,
        CLASS,
        MatchingStrategy::Equals,
    );
    let last = simple(
        ApplicationIdentifier::Title,
        "komorebi-e2e",
        MatchingStrategy::StartsWith,
    );
    let rules = [first.clone(), last.clone()];
    assert_eq!(
        should_act_window(TITLE, EXE, CLASS, PATH, &rules, &no_regex()),
        Some(last)
    );
    assert_eq!(
        should_act_window(TITLE, EXE, CLASS, PATH, &[first.clone()], &no_regex()),
        Some(first)
    );
}

// mirrors: rules / once the rules are cleared, a window they matched stays on the workspace in view
#[test]
fn no_rules_match_nothing() {
    assert_eq!(
        should_act_window(TITLE, EXE, CLASS, PATH, &[], &no_regex()),
        None
    );
}

// mirrors: title_rule / a window the ignore rule matches by its title is left alone
// mirrors: title_rule / it gives itself another title (the same rule no longer matches)
#[test]
fn a_rule_by_title_follows_the_title_it_is_asked_about() {
    let rule = simple(
        ApplicationIdentifier::Title,
        "secret",
        MatchingStrategy::Contains,
    );
    let ask =
        |title: &str| should_act_window(title, EXE, CLASS, PATH, &[rule.clone()], &no_regex());
    assert!(ask("a secret window").is_some());
    assert!(ask("a renamed window").is_none());
}

fn added(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::Show(
        SystemNotification::KWin(KWinNotification::WindowAdded),
        Window::from(id),
    )
}

// mirrors: manage_rules / a window an ignore rule by <kind> matches is not managed
// mirrors: manage_rules / with a manage rule by <kind> too, a window it matches is managed
// mirrors: manage_rules / ... and tiled: <base> gives it room
#[test]
fn a_manage_rule_rescues_a_window_an_ignore_rule_covers() {
    let fake = Arc::new(FakeBackend::with_one_monitor());
    let (_sender, receiver) = crossbeam_channel::bounded(1);
    let socket = std::path::PathBuf::from(format!("/tmp/kmr-{}.sock", std::process::id()));
    let mut wm = with_backend(fake.clone(), || {
        let mut wm = WindowManager::new(receiver, Some(socket)).unwrap();
        wm.init().unwrap();
        wm
    });

    fake.open_window(10_901, "kwrite", "org.kde.kwrite", "ignored window");
    fake.open_window(10_902, "kwrite", "org.kde.kwrite", "rescued window");

    let send = |wm: &mut WindowManager, message| {
        with_backend(fake.clone(), || wm.process_command(message, Vec::new())).unwrap();
    };
    // the rule tables are process-wide and this is the only test of this file that adds to them
    send(
        &mut wm,
        SocketMessage::IgnoreRule(ApplicationIdentifier::Class, "org.kde.kwrite".to_string()),
    );

    with_backend(fake.clone(), || wm.process_event(added(10_901))).unwrap();
    assert!(!wm.focused_workspace().unwrap().contains_window(10_901));

    send(
        &mut wm,
        SocketMessage::ManageRule(ApplicationIdentifier::Title, "rescued window".to_string()),
    );
    with_backend(fake.clone(), || wm.process_event(added(10_902))).unwrap();

    assert!(wm.focused_workspace().unwrap().contains_window(10_902));
    assert!(!wm.focused_workspace().unwrap().contains_window(10_901));
}
