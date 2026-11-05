//! Finding a word across everything the session recorded.
//!
//! A user says "it happened when Slack woke up" or an error names a window; this looks for the text
//! in komorebi's log, the events, what happened to the machine, the annotations and, on request,
//! the OS trace, and returns where each hit is so it can be fed straight to `trace`, `code` or
//! `at`.
//!
//! [`Analysis::search`] is what the `search` command prints. A [`SearchQuery`] describes what to
//! look for and where in time; the answer is a [`SearchData`] of [`Hit`]s ordered by time, each
//! with a `reference` (`log:345`, `event:12`, ...) in the same notation the time arguments of the
//! other commands take.
//!
//! # Where it looks
//!
//! | `source` of a hit | looked at | `reference` | searched when |
//! |---|---|---|---|
//! | `komorebi-log` | each line's message, and its spans and module with the message | `log:<line>` | always |
//! | `events` | the event's one-line summary | `event:<seq>` | always |
//! | `recorder` | an incident's kind and detail, `kind: detail` | `incident:<number>` | always |
//! | `annotations` | an annotation's title, text and expected behavior | the annotation's id | always |
//! | `oslog:...` | the text of each OS trace record | `oslog:<file>:<line>` | [`SearchQuery::oslog`] |
//!
//! The OS trace is large, which is why it is opt-in and the only source a `Budget` limits. OS
//! trace records from either host are searched the same way.

use super::Analysis;
use super::budget::Budget;
use super::envelope;
use crate::annotations;
use crate::timeline;
use crate::timeline::Filter;
use color_eyre::eyre;
use regex::Regex;
use regex::RegexBuilder;
use schemars::JsonSchema;
use serde::Serialize;

/// The schema name that identifies a [`SearchData`] in an envelope (`search/1`); listed by the
/// `schema` command.
pub const SCHEMA_SEARCH: &str = "search/1";

/// Where a piece of text was found in the session.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Hit {
    /// `komorebi-log`, `events`, `recorder`, `annotations` or `oslog:...`. For an OS trace record
    /// it is the source of the record itself, such as `oslog:system` or `oslog:app:Ghostty`.
    pub source: String,
    /// When it happened, in seconds of video time.
    pub video_s: f64,
    /// When it happened, in nanoseconds on the host's master monotonic clock.
    pub mach_ns: u64,
    /// Something to give another command: `log:345`, `event:12`, `incident:2`, an annotation id
    /// or `oslog:<file>:<line>`.
    pub reference: String,
    /// The text that matched. For a log line it is the line's message; for an event, its summary;
    /// for an annotation, its title, text and expected behavior joined by spaces; for an OS trace
    /// record, the rendered record.
    pub text: String,
}

/// The hits for a [`SearchQuery`], ordered by time: the `data` of the `search` command.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct SearchData {
    /// The pattern that was searched for, as given.
    pub query: String,
    /// The hits of this page, ordered by time and then by reference.
    pub hits: Vec<Hit>,
    /// How many hits there were after the cursor, including any not returned.
    pub matching: usize,
    /// Where it looked: always `komorebi-log`, `events`, `recorder` and `annotations`, and
    /// `oslog` if the OS trace was searched.
    pub searched: Vec<String>,
}

/// A search: what to look for, where in time, and how much to return.
///
/// The default is an empty pattern, which [`Analysis::search`] refuses, so a query is built with
/// at least a `pattern`. The other defaults are: a literal, case-insensitive match, the whole
/// recording, no OS trace, from the first hit, with no limit.
#[derive(Debug, Clone, Default)]
pub struct SearchQuery {
    /// The text to look for. A literal substring, or a regular expression if `regex` is set.
    pub pattern: String,
    /// Whether `pattern` is a regular expression (the syntax of the `regex` crate). A regular
    /// expression may match anywhere in the text, and for a log line it is tried against the
    /// message alone first, so an anchor such as `^processing` applies to the start of the message.
    pub regex: bool,
    /// Whether upper and lower case must match. Off by default.
    pub case_sensitive: bool,
    /// Ignore anything earlier than this, in nanoseconds on the host's master monotonic clock.
    pub from_mach_ns: Option<u64>,
    /// Ignore anything later than this, in nanoseconds on the host's master monotonic clock.
    pub to_mach_ns: Option<u64>,
    /// Also search the OS trace (large). Off by default.
    pub oslog: bool,
    /// Hits already returned: this many of the earliest hits are skipped, so a caller pages by
    /// passing how many it has seen.
    pub skip: usize,
    /// The most hits to return; `None` for all of them.
    pub limit: Option<usize>,
}

enum Matcher {
    Literal {
        needle: String,
        case_sensitive: bool,
    },
    Pattern(Regex),
}

impl Matcher {
    fn new(query: &SearchQuery) -> eyre::Result<Self> {
        if query.pattern.is_empty() {
            return Err(envelope::usage("nothing to search for"));
        }

        if query.regex {
            return RegexBuilder::new(&query.pattern)
                .case_insensitive(!query.case_sensitive)
                .build()
                .map(Self::Pattern)
                .map_err(|error| envelope::usage(format!("not a valid pattern: {error}")));
        }

        Ok(Self::Literal {
            needle: if query.case_sensitive {
                query.pattern.clone()
            } else {
                query.pattern.to_lowercase()
            },
            case_sensitive: query.case_sensitive,
        })
    }

    fn matches(&self, text: &str) -> bool {
        match self {
            Self::Literal {
                needle,
                case_sensitive,
            } => {
                if *case_sensitive {
                    text.contains(needle.as_str())
                } else {
                    text.to_lowercase().contains(needle.as_str())
                }
            }
            Self::Pattern(pattern) => pattern.is_match(text),
        }
    }
}

impl Analysis {
    /// Searches the session for `query`, within the limits of `budget`.
    ///
    /// Hits from every source are put together, ordered by time (earliest first, ties by
    /// reference) and then paged with [`SearchQuery::skip`] and [`SearchQuery::limit`], so
    /// [`SearchData::matching`] counts what is left after the skip, whatever the limit.
    ///
    /// A log line is a hit if the pattern matches its message alone (so `^processing` means what
    /// it says) or its message together with its spans and module. A log line with no time, or
    /// one that cannot be placed on the master clock, is never a hit, because it cannot be placed
    /// in a range. The time range applies to every source.
    ///
    /// `budget` limits only the OS trace, the one source that can be gigabytes: every record read
    /// is charged against it. The rest of the search does not consult it.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    ///
    /// - the pattern is empty (the usage exit kind);
    /// - `query.regex` is set and the pattern is not a valid regular expression (the usage exit
    ///   kind);
    /// - the annotations file exists and cannot be read;
    /// - `query.oslog` is set and the timeline index (`timeline.jsonl`) is missing, in which case
    ///   `komorebi-recorder reindex` rebuilds it, or an OS trace file cannot be read;
    /// - `query.oslog` is set and `budget` runs out (time, bytes or cancellation) while records are
    ///   read (the budget exit kind). No partial result is returned: narrow the range with
    ///   [`SearchQuery::from_mach_ns`] and [`SearchQuery::to_mach_ns`] and search again.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::Analysis;
    /// use komorebi_recorder::analysis::budget::Budget;
    /// use komorebi_recorder::analysis::search::SearchQuery;
    /// use komorebi_recorder::testkit::SessionBuilder;
    /// use serde_json::json;
    ///
    /// let (session, _) = SessionBuilder::new("doc-search")
    ///     .log(2.0, "INFO", "komorebi::reaper: reaping window: 4711")
    ///     .event(3.0, json!({"type": "FocusWorkspaceNumber", "content": 1}), None)
    ///     .incident(4.0, "display-disconnected", "display 2 was unplugged")
    ///     .build();
    /// let analysis = Analysis::open(&session.dir)?;
    ///
    /// let found = analysis.search(
    ///     &SearchQuery { pattern: "WINDOW".to_string(), ..SearchQuery::default() },
    ///     &Budget::unlimited(),
    /// )?;
    /// assert_eq!(found.matching, 1);
    /// assert_eq!(found.hits[0].source, "komorebi-log");
    /// assert_eq!(found.hits[0].reference, "log:1");
    ///
    /// // case matters when asked to, and nothing in the recording contains `WINDOW`
    /// let found = analysis.search(
    ///     &SearchQuery {
    ///         pattern: "WINDOW".to_string(),
    ///         case_sensitive: true,
    ///         ..SearchQuery::default()
    ///     },
    ///     &Budget::unlimited(),
    /// )?;
    /// assert!(found.hits.is_empty());
    ///
    /// // hits from different sources come back in time order, each with a reference to follow
    /// let found = analysis.search(
    ///     &SearchQuery {
    ///         pattern: r"workspace|unplugged|reaping".to_string(),
    ///         regex: true,
    ///         ..SearchQuery::default()
    ///     },
    ///     &Budget::unlimited(),
    /// )?;
    /// let references: Vec<_> = found.hits.iter().map(|hit| hit.reference.as_str()).collect();
    /// assert_eq!(references, ["log:1", "event:1", "incident:1"]);
    ///
    /// // an empty pattern is a usage error
    /// assert!(analysis.search(&SearchQuery::default(), &Budget::unlimited()).is_err());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// Searching with `oslog` set needs OS trace files and a timeline index. It is covered, with
    /// paging by cursor, by
    /// `tests/brief.rs::search_finds_words_across_sources_and_gives_references` and
    /// `tests/brief.rs::search_pages_with_a_cursor`.
    pub fn search(&self, query: &SearchQuery, budget: &Budget) -> eyre::Result<SearchData> {
        let matcher = Matcher::new(query)?;
        let in_range = |mach_ns: u64| {
            query.from_mach_ns.is_none_or(|from| mach_ns >= from)
                && query.to_mach_ns.is_none_or(|to| mach_ns <= to)
        };
        let mut hits: Vec<Hit> = Vec::new();
        let mut searched = vec!["komorebi-log", "events", "recorder", "annotations"];

        for line in &self.log().lines {
            let Some(mach_ns) = line
                .wall_us
                .and_then(|wall_us| self.clock().wall_to_mach(wall_us))
            else {
                continue;
            };
            let spans: Vec<&str> = line.spans.iter().map(|span| span.name.as_str()).collect();
            let haystack = format!(
                "{} {}: {}",
                spans.join(":"),
                line.target.as_deref().unwrap_or(""),
                line.message
            );
            // the message alone, so `^processing` anchors mean what they say, or with its spans and module
            if in_range(mach_ns) && (matcher.matches(&line.message) || matcher.matches(&haystack)) {
                hits.push(Hit {
                    source: "komorebi-log".to_string(),
                    video_s: self.video_s(mach_ns),
                    mach_ns,
                    reference: format!("log:{}", line.number),
                    text: line.message.clone(),
                });
            }
        }

        for event in self.events() {
            let haystack = format!(
                "{} {}",
                event.info.summary(),
                event.info.argument.as_deref().unwrap_or("")
            );
            if in_range(event.recv_mach_ns) && matcher.matches(&haystack) {
                hits.push(Hit {
                    source: "events".to_string(),
                    video_s: self.video_s(event.recv_mach_ns),
                    mach_ns: event.recv_mach_ns,
                    reference: format!("event:{}", event.seq),
                    text: event.info.summary(),
                });
            }
        }

        for incident in self.incidents() {
            let mach_ns = self.manifest.mach_at_video_seconds(incident.video_s);
            let text = format!("{}: {}", incident.kind, incident.detail);
            if in_range(mach_ns) && matcher.matches(&text) {
                hits.push(Hit {
                    source: "recorder".to_string(),
                    video_s: incident.video_s,
                    mach_ns,
                    reference: format!("incident:{}", incident.number),
                    text,
                });
            }
        }

        for annotation in annotations::load(&self.session.dir)?.annotations {
            let text = format!(
                "{} {} {}",
                annotation.title,
                annotation.text,
                annotation.expected.clone().unwrap_or_default()
            );
            if in_range(annotation.start_mach_ns) && matcher.matches(&text) {
                hits.push(Hit {
                    source: "annotations".to_string(),
                    video_s: self.video_s(annotation.start_mach_ns),
                    mach_ns: annotation.start_mach_ns,
                    reference: annotation.id.clone(),
                    text: text.trim().to_string(),
                });
            }
        }

        if query.oslog {
            searched.push("oslog");
            let from_s = query
                .from_mach_ns
                .map_or(f64::MIN / 2.0, |ns| self.video_s(ns));
            let to_s = query
                .to_mach_ns
                .map_or(f64::MAX / 2.0, |ns| self.video_s(ns));
            // the timeline index reads each record's raw text; stop when the budget says to
            let records = timeline::query(
                &self.session,
                &self.manifest,
                from_s.max(-1e6),
                to_s.min(1e9),
                &Filter {
                    app: None,
                    sources: vec!["oslog".to_string()],
                },
            )?;
            for record in records {
                budget
                    .charge(record.text.len() as u64)
                    .map_err(|exceeded| {
                        envelope::over_budget(format!(
                            "the search {exceeded}; narrow it with --from/--to"
                        ))
                    })?;
                if matcher.matches(&record.text) {
                    hits.push(Hit {
                        source: record.entry.source.clone(),
                        video_s: record.entry.video_s,
                        mach_ns: record.entry.mach_ns,
                        reference: format!("oslog:{}:{}", record.entry.file, record.entry.line),
                        text: record.text,
                    });
                }
            }
        }

        hits.sort_by(|a, b| {
            a.mach_ns
                .cmp(&b.mach_ns)
                .then(a.reference.cmp(&b.reference))
        });
        let matching = hits.len().saturating_sub(query.skip);
        let page: Vec<Hit> = hits
            .into_iter()
            .skip(query.skip)
            .take(query.limit.unwrap_or(usize::MAX))
            .collect();

        Ok(SearchData {
            query: query.pattern.clone(),
            hits: page,
            matching,
            searched: searched.into_iter().map(str::to_string).collect(),
        })
    }
}
