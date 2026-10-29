//! A rule for komorebi-application-specific-configuration, and putting it in the file.
//!
//! The repository keeps its rules in `applications.json` (komorebi for Windows) and
//! `applications.mac.json` (komorebi for Mac), one entry per application, as `jq` prints them:
//! two-space indentation, the entries sorted by name with case ignored (`just sort` does it), and
//! each rule under the kind of rule it is (`ignore`, `manage`, `floating`...). A pull request that
//! reprints the whole file is a pull request nobody can read, so [`Rule::apply`] changes the text
//! of the file where the rule goes and nowhere else, and then checks that what it made is the old
//! file and the rule and nothing more.
//!
//! # Example
//!
//! ```rust
//! use komorebi_recorder::asc::rule::Applied;
//! use komorebi_recorder::asc::rule::Identifier;
//! use komorebi_recorder::asc::rule::Rule;
//!
//! let rule = Rule::ignore(
//!     "DDM",
//!     vec![Identifier::exe("DDM.exe"), Identifier::class("DDMWindow")],
//! );
//! let file = "{\n  \"$schema\": \"https://example.com/schema.json\",\n  \"Ableton Live\": {\n    \"ignore\": [\n      {\n        \"kind\": \"Class\",\n        \"id\": \"Vst3PlugWindow\",\n        \"matching_strategy\": \"Legacy\"\n      }\n    ]\n  }\n}\n";
//!
//! let Applied::Changed(text) = rule.apply(file)? else {
//!     panic!("the rule was not there");
//! };
//! assert!(text.contains("\"DDM\": {"));
//! // the entry goes after the one that sorts before it, and the rest is as it was
//! assert!(text.starts_with(&file[..file.len() - 4]));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use crate::analysis::ghost::GhostTile;
use crate::asc::Platform;
use serde_json::Value;
use serde_json::json;

/// What a rule asks about a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identifier {
    /// What of the window is looked at: `Exe`, `Class`, `Title` or `Path`.
    pub kind: &'static str,
    /// What it is looked for as.
    pub id: String,
    /// How it is matched: `Equals`, `StartsWith`, `EndsWith`, `Contains`, `Regex`...
    pub strategy: &'static str,
}

impl Identifier {
    /// The program is exactly `exe`.
    #[must_use]
    pub fn exe(exe: &str) -> Self {
        Self {
            kind: "Exe",
            id: exe.to_string(),
            strategy: "Equals",
        }
    }

    /// The window class is exactly `class`.
    #[must_use]
    pub fn class(class: &str) -> Self {
        Self {
            kind: "Class",
            id: class.to_string(),
            strategy: "Equals",
        }
    }

    /// The window's title is exactly `title`.
    #[must_use]
    pub fn title(title: &str) -> Self {
        Self {
            kind: "Title",
            id: title.to_string(),
            strategy: "Equals",
        }
    }

    fn value(&self) -> Value {
        json!({"kind": self.kind, "id": self.id, "matching_strategy": self.strategy})
    }

    /// The identifier as `jq` prints it, from `indent` spaces in: the keys in the order the
    /// repository has them (a `serde_json` map would sort them).
    fn text(&self, indent: usize) -> String {
        let pad = " ".repeat(indent);
        format!(
            "{pad}{{\n{pad}  \"kind\": {},\n{pad}  \"id\": {},\n{pad}  \"matching_strategy\": {}\n{pad}}}",
            quoted(self.kind),
            quoted(&self.id),
            quoted(self.strategy),
        )
    }
}

fn quoted(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string())
}

/// A rule that ignores windows: not one of komorebi's tiles, and not touched by komorebi at all.
///
/// More than one identifier means a composite rule, which matches a window only when all of them
/// do (the nested array of `applications.json`): the way to ignore one window of an application and
/// not the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// The entry's name in the file: the application.
    pub app: String,
    /// What a window has to be, all of it.
    pub identifiers: Vec<Identifier>,
}

/// What [`Rule::apply`] came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Applied {
    /// The file with the rule in it.
    Changed(String),
    /// The file has the rule already.
    AlreadyThere,
}

/// Why [`Rule::apply`] did not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyError {
    /// The file is not JSON, or is not an object of applications.
    NotAnObject(String),
    /// The file is not laid out the way `jq` prints it, so the rule cannot be put in without
    /// reprinting the file.
    Layout(String),
}

impl std::fmt::Display for ApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnObject(why) => {
                write!(f, "the file is not a JSON object of applications: {why}")
            }
            Self::Layout(why) => write!(
                f,
                "the file is not laid out as `jq` prints it, so the rule cannot be added to it: {why}"
            ),
        }
    }
}

impl std::error::Error for ApplyError {}

/// Why there is no rule to write for a ghost tile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoRule(pub String);

impl std::fmt::Display for NoRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NoRule {}

impl Rule {
    /// A rule that ignores the windows that match every one of `identifiers`, as the entry `app`.
    #[must_use]
    pub fn ignore(app: &str, identifiers: Vec<Identifier>) -> Self {
        Self {
            app: app.to_string(),
            identifiers,
        }
    }

    /// The rule that ignores a ghost tile, and only that: as narrow as it has to be to leave the
    /// rest of the application's windows alone.
    ///
    /// It is always a composite rule, so that it is never the whole program that is ignored:
    ///
    /// - **Windows**: the program and the class. If another window of the program that the
    ///   recording saw has the same class, the class does not tell them apart, and the title is
    ///   added.
    /// - **Mac**: the program (the application's name) and the title, which is all a Mac window has
    ///   to tell it from the application's others.
    ///
    /// # Errors
    ///
    /// Returns [`NoRule`] when there is not enough to write a rule that catches the ghost and
    /// nothing the application is for: komorebi did not report its program, or its class on
    /// Windows, or it has no title where the title is needed, or a window of the program that was on
    /// screen looks exactly like it.
    pub fn for_ghost(ghost: &GhostTile) -> Result<Self, NoRule> {
        if ghost.exe.is_empty() {
            return Err(NoRule(
                "komorebi did not report which program the window belongs to".to_string(),
            ));
        }
        let mut identifiers = vec![Identifier::exe(&ghost.exe)];

        match ghost.platform {
            Platform::Windows => {
                if ghost.class.is_empty() {
                    return Err(NoRule(
                        "komorebi did not report the window's class, and a rule on the program \
                         alone would ignore all of its windows"
                            .to_string(),
                    ));
                }
                identifiers.push(Identifier::class(&ghost.class));
                let shared = ghost
                    .siblings
                    .iter()
                    .any(|sibling| sibling.class == ghost.class);
                if shared {
                    // the program's windows that are on screen have this class too
                    if ghost.title.is_empty()
                        || ghost
                            .siblings
                            .iter()
                            .any(|sibling| sibling.title == ghost.title)
                    {
                        return Err(NoRule(format!(
                            "windows of {} that are on screen have the same class, and nothing \
                             (its title is {}) tells them from the ghost",
                            ghost.exe,
                            if ghost.title.is_empty() {
                                "empty".to_string()
                            } else {
                                format!("{:?}, the same as theirs", ghost.title)
                            }
                        )));
                    }
                    identifiers.push(Identifier::title(&ghost.title));
                }
            }
            Platform::Macos => {
                if ghost.title.is_empty() {
                    return Err(NoRule(
                        "the window has no title, and on a Mac the title is all there is to tell it \
                         from the application's other windows"
                            .to_string(),
                    ));
                }
                if ghost
                    .siblings
                    .iter()
                    .any(|sibling| sibling.title == ghost.title)
                {
                    return Err(NoRule(format!(
                        "a window of {} that is on screen has the same title ({:?})",
                        ghost.exe, ghost.title
                    )));
                }
                identifiers.push(Identifier::title(&ghost.title));
            }
            Platform::Kde => {
                return Err(NoRule(
                    "the rules of komorebi for KDE are in its own repository, and a rule for it is \
                     not written here yet"
                        .to_string(),
                ));
            }
        }

        Ok(Self::ignore(&app_name(&ghost.exe), identifiers))
    }

    /// The rule as the value it is in the file: one identifier alone, or an array of them.
    fn value(&self) -> Value {
        match self.identifiers.as_slice() {
            [one] => one.value(),
            many => Value::Array(many.iter().map(Identifier::value).collect()),
        }
    }

    /// The rule as the file would have it, from `indent` spaces in.
    fn text(&self, indent: usize) -> String {
        match self.identifiers.as_slice() {
            [one] => one.text(indent),
            many => {
                let pad = " ".repeat(indent);
                let items: Vec<String> = many.iter().map(|item| item.text(indent + 2)).collect();
                format!("{pad}[\n{}\n{pad}]", items.join(",\n"))
            }
        }
    }

    /// The entry as a reader sees it, to read before it is sent: what goes into `platform`'s file.
    #[must_use]
    pub fn preview(&self) -> String {
        format!(
            "  {}: {{\n    \"ignore\": [\n{}\n    ]\n  }}",
            quoted(&self.app),
            self.text(6)
        )
    }

    /// Returns the file with this rule in it.
    ///
    /// The entry for [`Rule::app`] is added where it sorts, or, if the file has one already, the
    /// rule is added to its `ignore` list, which is made if it has none. The result is checked: it
    /// must be JSON, and the same JSON as `file` with the rule added.
    ///
    /// # Errors
    ///
    /// Returns an error when `file` is not a JSON object, or is not laid out as `jq` prints it
    /// (two-space indentation, entries one to a block): the rule is then not added, so as not to
    /// reprint a file whose layout is not understood.
    pub fn apply(&self, file: &str) -> Result<Applied, ApplyError> {
        let original: Value = serde_json::from_str(file)
            .map_err(|error| ApplyError::NotAnObject(error.to_string()))?;
        let Some(entries) = original.as_object() else {
            return Err(ApplyError::NotAnObject("it is not an object".to_string()));
        };

        let rule = self.value();
        let existing = entries.get(&self.app);
        if existing
            .and_then(|entry| entry.get("ignore"))
            .and_then(Value::as_array)
            .is_some_and(|rules| rules.contains(&rule))
        {
            return Ok(Applied::AlreadyThere);
        }

        let lines: Vec<&str> = file.split_inclusive('\n').collect();
        let found = blocks(&lines)?;
        let mut out: Vec<String> = lines.iter().map(|line| (*line).to_string()).collect();

        if let Some(block) = found.iter().find(|block| block.name == self.app) {
            self.add_to(&mut out, &lines, block)?;
        } else {
            self.add_entry(&mut out, &found)?;
        }
        let text = out.concat();

        // what was made is the file and the rule, and nothing else
        let mut expected = original;
        if let Some(rules) = expected
            .as_object_mut()
            .and_then(|entries| {
                entries
                    .entry(self.app.clone())
                    .or_insert_with(|| json!({}))
                    .as_object_mut()
            })
            .and_then(|entry| {
                entry
                    .entry("ignore")
                    .or_insert_with(|| json!([]))
                    .as_array_mut()
            })
        {
            rules.push(rule);
        }
        let made: Value = serde_json::from_str(&text)
            .map_err(|error| ApplyError::Layout(format!("the result is not JSON ({error})")))?;
        if made != expected {
            return Err(ApplyError::Layout(
                "the result is not the file with the rule added".to_string(),
            ));
        }

        // a file that was sorted still is
        let names = |lines: &[&str]| -> Result<Vec<String>, ApplyError> {
            Ok(blocks(lines)?.into_iter().map(|block| block.name).collect())
        };
        let before = names(&lines)?;
        let made_lines: Vec<&str> = text.split_inclusive('\n').collect();
        let after = names(&made_lines)?;
        if is_sorted(&before) && !is_sorted(&after) {
            return Err(ApplyError::Layout(
                "the entries would no longer be sorted".to_string(),
            ));
        }
        Ok(Applied::Changed(text))
    }

    /// Adds the rule to an entry the file has.
    fn add_to(&self, out: &mut [String], lines: &[&str], block: &Block) -> Result<(), ApplyError> {
        let line = |at: usize| lines[at].trim_end_matches('\n');
        // the line that opens the entry's `ignore` list, if it has one
        let ignore_open = (block.start + 1..block.end).find(|at| line(*at) == "    \"ignore\": [");

        if let Some(open) = ignore_open {
            let close = (open + 1..block.end)
                .find(|at| matches!(line(*at), "    ]" | "    ],"))
                .ok_or_else(|| ApplyError::Layout("an `ignore` list is not closed".to_string()))?;
            if close > open + 1 {
                out[close - 1] = format!("{},\n", line(close - 1));
            }
            out[close] = format!("{}\n{}", self.text(6), out[close]);
            return Ok(());
        }

        // no `ignore` yet: it joins the entry's other rules, after the last of them
        let close = block.end;
        out[close - 1] = format!("{},\n", line(close - 1));
        out[close] = format!("    \"ignore\": [\n{}\n    ]\n{}", self.text(6), out[close]);
        Ok(())
    }

    /// Adds an entry for the application, where its name sorts.
    fn add_entry(&self, out: &mut [String], blocks: &[Block]) -> Result<(), ApplyError> {
        let entry = self.preview();
        let upper = self.app.to_ascii_uppercase();
        match blocks
            .iter()
            .find(|block| block.name.to_ascii_uppercase() > upper)
        {
            Some(next) => {
                out[next.start] = format!("{entry},\n{}", out[next.start]);
            }
            None => {
                let last = blocks
                    .last()
                    .ok_or_else(|| ApplyError::Layout("the file has no entries".to_string()))?;
                out[last.end] = format!("{},\n{entry}\n", out[last.end].trim_end_matches('\n'));
            }
        }
        Ok(())
    }
}

/// Whether the names are in the order `just sort` puts them in: case ignored.
fn is_sorted(names: &[String]) -> bool {
    let upper: Vec<String> = names.iter().map(|name| name.to_ascii_uppercase()).collect();
    upper.windows(2).all(|pair| pair[0] <= pair[1])
}

/// One application's entry in the file: its name and the lines it is on.
#[derive(Debug)]
struct Block {
    name: String,
    /// The line `  "name": {` is on.
    start: usize,
    /// The line `  }` or `  },` that closes it.
    end: usize,
}

/// The entries of the file, found by the layout `jq` gives them: a line `  "name": {` opens one
/// and the next `  }` or `  },` closes it.
fn blocks(lines: &[&str]) -> Result<Vec<Block>, ApplyError> {
    let mut found = Vec::new();
    let mut open: Option<(String, usize)> = None;
    for (at, raw) in lines.iter().enumerate() {
        if raw.ends_with("\r\n") {
            return Err(ApplyError::Layout("the lines end in CRLF".to_string()));
        }
        let line = raw.trim_end_matches('\n');
        match &open {
            None => {
                if let Some(rest) = line
                    .strip_prefix("  ")
                    .filter(|rest| rest.starts_with('"'))
                    .and_then(|rest| rest.strip_suffix(": {"))
                {
                    let name: String = serde_json::from_str(rest).map_err(|_| {
                        ApplyError::Layout(format!("line {} is not an entry", at + 1))
                    })?;
                    open = Some((name, at));
                }
            }
            Some((name, start)) => {
                if line == "  }" || line == "  }," {
                    found.push(Block {
                        name: name.clone(),
                        start: *start,
                        end: at,
                    });
                    open = None;
                }
            }
        }
    }
    if open.is_some() {
        return Err(ApplyError::Layout("an entry is not closed".to_string()));
    }
    Ok(found)
}

/// The name of an application for its program: `DDM.exe` is `DDM`.
#[must_use]
pub fn app_name(exe: &str) -> String {
    let stem = exe
        .strip_suffix(".exe")
        .or_else(|| exe.strip_suffix(".EXE"))
        .unwrap_or(exe);
    stem.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::ghost::Sibling;

    const FILE: &str = r#"{
  "$schema": "https://example.com/schema.json",
  "1Password": {
    "ignore": [
      {
        "kind": "Exe",
        "id": "1Password.exe",
        "matching_strategy": "Equals"
      }
    ]
  },
  "Ableton Live": {
    "ignore": [
      {
        "kind": "Class",
        "id": "Vst3PlugWindow",
        "matching_strategy": "Legacy"
      }
    ],
    "manage": [
      {
        "kind": "Exe",
        "id": "Live.exe",
        "matching_strategy": "Equals"
      }
    ]
  },
  "Zoom": {
    "floating": [
      {
        "kind": "Class",
        "id": "ZPContentViewWndClass",
        "matching_strategy": "Equals"
      }
    ]
  }
}
"#;

    fn ghost_rule(app: &str) -> Rule {
        Rule::ignore(
            app,
            vec![Identifier::exe("DDM.exe"), Identifier::class("DDMWindow")],
        )
    }

    fn changed(rule: &Rule, file: &str) -> String {
        match rule.apply(file).expect("apply") {
            Applied::Changed(text) => text,
            Applied::AlreadyThere => panic!("the rule was there already"),
        }
    }

    #[test]
    fn a_new_application_goes_where_it_sorts_and_nothing_else_moves() {
        let text = changed(&ghost_rule("DDM"), FILE);

        let a = text.find("\"Ableton Live\"").expect("ableton");
        let d = text.find("\"DDM\"").expect("ddm");
        let z = text.find("\"Zoom\"").expect("zoom");
        assert!(a < d && d < z, "{text}");
        // every line of the old file is still there, in order, and the new entry is the only
        // addition
        let added: Vec<&str> = text
            .lines()
            .filter(|line| !FILE.lines().any(|old| old == *line))
            .collect();
        assert!(
            added.iter().any(|line| line.contains("\"DDMWindow\"")),
            "{added:?}"
        );
        let old_lines: Vec<&str> = FILE.lines().collect();
        let kept: Vec<&str> = text
            .lines()
            .filter(|line| old_lines.contains(line))
            .collect();
        assert!(kept.len() >= old_lines.len() - 1, "{text}");
        assert!(text.ends_with("}\n"));
    }

    #[test]
    fn sorting_ignores_case_like_just_sort() {
        // `ddm` sorts after `Ableton Live` and before `Zoom` whatever its case
        let text = changed(&ghost_rule("ddm"), FILE);

        let d = text.find("\"ddm\"").expect("ddm");
        assert!(text.find("\"Ableton Live\"").expect("a") < d);
        assert!(d < text.find("\"Zoom\"").expect("z"));
    }

    #[test]
    fn an_application_that_sorts_last_is_added_at_the_end_with_the_comma_moved() {
        let text = changed(&ghost_rule("ZZZ"), FILE);

        assert!(text.contains("  },\n  \"ZZZ\": {"), "{text}");
        assert!(text.ends_with("    ]\n  }\n}\n"), "{text}");
    }

    #[test]
    fn an_application_that_sorts_first_is_added_before_the_first_entry() {
        let text = changed(&ghost_rule("0Day"), FILE);

        assert!(text.find("\"0Day\"").expect("0day") < text.find("\"1Password\"").expect("1p"));
        assert!(text.find("\"$schema\"").expect("schema") < text.find("\"0Day\"").expect("0day"));
    }

    #[test]
    fn an_application_the_file_has_gets_the_rule_in_its_ignore_list() {
        let rule = Rule::ignore(
            "Ableton Live",
            vec![Identifier::exe("Live.exe"), Identifier::class("Ghost")],
        );

        let text = changed(&rule, FILE);

        let value: Value = serde_json::from_str(&text).expect("json");
        assert_eq!(
            value["Ableton Live"]["ignore"].as_array().map(Vec::len),
            Some(2)
        );
        assert_eq!(
            value["Ableton Live"]["manage"].as_array().map(Vec::len),
            Some(1)
        );
    }

    #[test]
    fn an_application_with_no_ignore_list_gets_one() {
        let rule = Rule::ignore("Zoom", vec![Identifier::class("Ghost")]);

        let text = changed(&rule, FILE);

        let value: Value = serde_json::from_str(&text).expect("json");
        assert_eq!(value["Zoom"]["ignore"].as_array().map(Vec::len), Some(1));
        assert_eq!(value["Zoom"]["floating"].as_array().map(Vec::len), Some(1));
        // a single identifier is not wrapped in an array
        assert_eq!(value["Zoom"]["ignore"][0]["id"], "Ghost");
    }

    #[test]
    fn a_rule_the_file_has_is_not_added_again() {
        let rule = Rule::ignore("1Password", vec![Identifier::exe("1Password.exe")]);

        assert_eq!(rule.apply(FILE), Ok(Applied::AlreadyThere));
    }

    #[test]
    fn a_composite_rule_is_the_nested_array_the_file_uses() {
        let text = changed(&ghost_rule("DDM"), FILE);

        let value: Value = serde_json::from_str(&text).expect("json");
        let rule = &value["DDM"]["ignore"][0];
        assert_eq!(rule[0]["kind"], "Exe");
        assert_eq!(rule[1]["kind"], "Class");
        assert_eq!(rule[1]["matching_strategy"], "Equals");
        // and the keys are in the order of the rest of the file
        assert!(
            text.contains("\"kind\": \"Exe\",\n          \"id\": \"DDM.exe\""),
            "{text}"
        );
    }

    #[test]
    fn a_file_that_is_not_laid_out_like_jq_prints_it_is_refused() {
        let error = ghost_rule("DDM")
            .apply("{\"A\":{\"ignore\":[]}}")
            .expect_err("compact");

        assert!(matches!(error, ApplyError::Layout(_)), "{error:?}");
        assert!(matches!(
            ghost_rule("DDM").apply("[]"),
            Err(ApplyError::NotAnObject(_))
        ));
        assert!(matches!(
            ghost_rule("DDM").apply("not json"),
            Err(ApplyError::NotAnObject(_))
        ));
    }

    #[test]
    fn what_is_quoted_is_escaped() {
        let rule = Rule::ignore("A \"quoted\" app", vec![Identifier::class("a\\b")]);

        let text = changed(&rule, FILE);

        let value: Value = serde_json::from_str(&text).expect("json");
        assert_eq!(value["A \"quoted\" app"]["ignore"][0]["id"], "a\\b");
    }

    fn ghost(platform: Platform, exe: &str, class: &str, title: &str) -> GhostTile {
        GhostTile {
            platform,
            hwnd: 1,
            exe: exe.into(),
            class: class.into(),
            title: title.into(),
            hidden_how: "cloaked".into(),
            place: String::new(),
            reads: 2,
            first_seq: 1,
            first_video_s: 0.0,
            last_video_s: 1.0,
            persisted_to_end: true,
            siblings: Vec::new(),
        }
    }

    fn sibling(class: &str, title: &str) -> Sibling {
        Sibling {
            id: 2,
            class: class.into(),
            title: title.into(),
        }
    }

    fn kinds(rule: &Rule) -> Vec<(&'static str, String)> {
        rule.identifiers
            .iter()
            .map(|identifier| (identifier.kind, identifier.id.clone()))
            .collect()
    }

    #[test]
    fn a_ghost_on_windows_is_ignored_by_its_program_and_class_together() {
        let ghost = ghost(Platform::Windows, "DDM.exe", "DDMWindow", "");

        let rule = Rule::for_ghost(&ghost).expect("rule");

        assert_eq!(rule.app, "DDM");
        assert_eq!(
            kinds(&rule),
            [
                ("Exe", "DDM.exe".to_string()),
                ("Class", "DDMWindow".to_string())
            ]
        );
    }

    #[test]
    fn a_program_that_has_windows_of_another_class_keeps_the_rule_on_the_ghosts_class() {
        let mut ghost = ghost(Platform::Windows, "chrome.exe", "Chrome_Helper", "");
        ghost.siblings = vec![sibling("Chrome_WidgetWin_1", "A page")];

        let rule = Rule::for_ghost(&ghost).expect("rule");

        // the program's real windows are another class: the class already tells them apart
        assert_eq!(rule.identifiers.len(), 2);
    }

    #[test]
    fn a_class_the_programs_real_windows_share_is_told_apart_by_the_title() {
        let mut ghost = ghost(Platform::Windows, "app.exe", "Frame", "Update helper");
        ghost.siblings = vec![sibling("Frame", "Main window")];

        let rule = Rule::for_ghost(&ghost).expect("rule");

        assert_eq!(
            kinds(&rule),
            [
                ("Exe", "app.exe".to_string()),
                ("Class", "Frame".to_string()),
                ("Title", "Update helper".to_string()),
            ]
        );
    }

    #[test]
    fn nothing_is_written_that_would_ignore_what_is_on_screen() {
        let mut shared_class_and_no_title = ghost(Platform::Windows, "app.exe", "Frame", "");
        shared_class_and_no_title.siblings = vec![sibling("Frame", "Main window")];
        let mut same_title = ghost(Platform::Windows, "app.exe", "Frame", "Main window");
        same_title.siblings = vec![sibling("Frame", "Main window")];

        for ghost in [shared_class_and_no_title, same_title] {
            let error = Rule::for_ghost(&ghost).expect_err("it would ignore real windows");
            assert!(error.0.contains("tells them from the ghost"), "{error}");
        }
    }

    #[test]
    fn a_ghost_whose_class_is_not_known_has_no_rule() {
        let error = Rule::for_ghost(&ghost(Platform::Windows, "DDM.exe", "", "")).expect_err("no");
        assert!(error.0.contains("ignore all of its windows"), "{error}");

        let error = Rule::for_ghost(&ghost(Platform::Windows, "", "Class", "")).expect_err("no");
        assert!(error.0.contains("which program"), "{error}");
    }

    #[test]
    fn a_ghost_on_a_mac_is_ignored_by_its_application_and_title() {
        let mut ghost = ghost(Platform::Macos, "Safari", "", "Reader Helper");
        ghost.siblings = vec![sibling("", "Apple")];

        let rule = Rule::for_ghost(&ghost).expect("rule");

        assert_eq!(rule.app, "Safari");
        assert_eq!(
            kinds(&rule),
            [
                ("Exe", "Safari".to_string()),
                ("Title", "Reader Helper".to_string()),
            ]
        );
    }

    #[test]
    fn on_a_mac_a_title_is_needed_and_must_not_be_the_title_of_a_window_on_screen() {
        let error = Rule::for_ghost(&ghost(Platform::Macos, "Safari", "", "")).expect_err("no");
        assert!(error.0.contains("no title"), "{error}");

        let mut same = ghost(Platform::Macos, "Safari", "", "Apple");
        same.siblings = vec![sibling("", "Apple")];
        let error = Rule::for_ghost(&same).expect_err("no");
        assert!(error.0.contains("same title"), "{error}");
    }

    #[test]
    fn a_program_loses_its_exe_in_its_name() {
        assert_eq!(app_name("WindowsTerminal.exe"), "WindowsTerminal");
        assert_eq!(app_name("Safari"), "Safari");
    }
}
