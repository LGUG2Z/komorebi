//! `annotate` and `annotations`: marking where a bug starts and ends, notes, and agent findings.
//!
//! `annotate <session> add|edit|delete` writes to the session's `annotations.jsonl` (see
//! [`crate::annotations`], which does the writing and holds the format) and `annotations <session>`
//! lists what is there. Both work on a session that is still being recorded, so a bug can be marked
//! while it is happening.
//!
//! This file is where the rules about what an annotation must say are enforced, since
//! [`crate::annotations::add`] itself only refuses an end before a start:
//!
//! | Kind | Must have |
//! |---|---|
//! | bug | `--text`, and `--end` or `--open-ended` |
//! | note | `--text` |
//! | finding | `--text` and at least one `--ref` |
//!
//! The start is `--start`; failing that, the start of the annotation named by `--about`; for a
//! finding, failing that, the earliest of its evidence that has a time (an `event:` or a `frame:`
//! reference); otherwise `--start` is required. A finding with no `--end` takes the end of the bug
//! it is `--about`. The author is `user`, or `agent` for a finding, unless `--author` says
//! otherwise.
//!
//! Every `--ref` is checked to exist before anything is written
//! ([`crate::annotations::validate_refs`]), and a reference that cannot be checked (komorebi's
//! source was not found) is a warning, not a refusal. Times take any form `state --at` takes (see
//! `analysis::timespec`). An annotation is printed back as it reads after the change
//! ([`crate::report::annotation_text`], or the `annotation/1` envelope with `--json`).
//!
//! Covered by `tests/annotate.rs`, which runs the commands through the real binary.

use super::print_json;
use crate::analysis::answers::AnnotationData;
use crate::analysis::answers::AnnotationsData;
use crate::analysis::answers::DeletedData;
use crate::analysis::envelope;
use crate::analysis::envelope::Envelope;
use crate::analysis::lookup::SessionLookup;
use crate::analysis::timespec::TimeSpec;
use crate::annotations;
use crate::annotations::AnnotationView;
use crate::annotations::Kind;
use crate::annotations::New;
use crate::annotations::Patch;
use crate::annotations::Ref;
use crate::report;
use crate::session::Manifest;
use crate::session::Session;
use clap::Args;
use clap::Subcommand;
use clap::ValueEnum;
use color_eyre::eyre;
use std::path::PathBuf;

/// The kind of annotation, as the `--kind` option spells it.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum KindArg {
    /// A region of the recording where the bug is visible, with what the user saw
    Bug,
    /// A remark about a moment or region
    Note,
    /// A conclusion about the cause of a bug, with the evidence for it
    Finding,
}

impl From<KindArg> for Kind {
    fn from(kind: KindArg) -> Self {
        match kind {
            KindArg::Bug => Self::Bug,
            KindArg::Note => Self::Note,
            KindArg::Finding => Self::Finding,
        }
    }
}

/// The arguments of `annotate`: the session and what to do to its annotations.
#[derive(Args, Debug)]
pub struct AnnotateArgs {
    /// Session directory (it may still be recording)
    pub session: PathBuf,
    /// What to do: `add`, `edit` or `delete`
    #[command(subcommand)]
    pub action: Action,
}

/// The operations of `annotate` on the session's annotations.
#[derive(Subcommand, Debug)]
pub enum Action {
    /// Add a bug region, a note or a finding
    Add(AddArgs),
    /// Change an annotation
    Edit(EditArgs),
    /// Remove an annotation
    Delete(DeleteArgs),
}

/// The arguments of `annotate add`.
#[derive(Args, Debug)]
pub struct AddArgs {
    /// What kind of annotation this is
    #[arg(long, value_enum)]
    pub kind: KindArg,
    /// Where it starts: 12.5, 1:02.5, wall:<RFC 3339>, event:<seq>, bug:<id>.end, optionally +3s.
    /// A finding defaults to the start of the bug it is --about
    #[arg(long, value_name = "TIME")]
    pub start: Option<String>,
    /// Where it ends (same forms). A bug needs this or --open-ended
    #[arg(long, value_name = "TIME")]
    pub end: Option<String>,
    /// The bug was still happening when the recording stopped
    #[arg(long, conflicts_with = "end")]
    pub open_ended: bool,
    /// What it looks like (a bug), the remark (a note) or the conclusion (a finding)
    #[arg(long)]
    pub text: Option<String>,
    /// A short title
    #[arg(long)]
    pub title: Option<String>,
    /// What should have happened instead
    #[arg(long)]
    pub expected: Option<String>,
    /// Only on this display (repeatable)
    #[arg(long = "display", value_name = "ID")]
    pub displays: Vec<u32>,
    /// Who is saying this [default: user, or agent for a finding]
    #[arg(long)]
    pub author: Option<String>,
    /// For a finding: the bug annotation it explains
    #[arg(long, value_name = "ID")]
    pub about: Option<String>,
    /// Evidence (repeatable; a finding needs at least one): event:<seq>, log:<line>,
    /// log:<file>:<line>, code:<path>:<line>, frame:<time>. A log line can also be named by its
    /// text, log:"some text" (quote it for the shell: 'log:"some text"'), by its UTC time,
    /// log:HH:MM:SS.mmm, or by a video time, log:12.5; the line it resolves to is printed to check
    #[arg(long = "ref", value_name = "REF")]
    pub refs: Vec<String>,
    /// A checkout of the recorded komorebi, to check code: references [default: `$KOMOREBI_MAC_SRC`
    /// or `$KOMOREBI_WINDOWS_SRC` for the session's komorebi, `$KOMOREBI_SOURCE`, or this build's]
    #[arg(long = "komorebi-src", alias = "code-root", value_name = "DIR")]
    pub code_root: Option<PathBuf>,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

/// The arguments of `annotate edit`: the id, and the fields to change.
#[derive(Args, Debug)]
pub struct EditArgs {
    /// The annotation id, e.g. a-3f9c1e02
    pub id: String,
    /// Move the start (same forms as `add --start`)
    #[arg(long, value_name = "TIME")]
    pub start: Option<String>,
    /// An end can be moved but not removed
    #[arg(long, value_name = "TIME")]
    pub end: Option<String>,
    /// Replace the description
    #[arg(long)]
    pub text: Option<String>,
    /// Replace the title
    #[arg(long)]
    pub title: Option<String>,
    /// Replace what should have happened instead (an empty text clears it)
    #[arg(long)]
    pub expected: Option<String>,
    /// Replace the displays (repeatable)
    #[arg(long = "display", value_name = "ID")]
    pub displays: Vec<u32>,
    /// Replace the evidence (repeatable)
    #[arg(long = "ref", value_name = "REF")]
    pub refs: Vec<String>,
    /// A checkout of the recorded komorebi, to check code: references [default: `$KOMOREBI_MAC_SRC`
    /// or `$KOMOREBI_WINDOWS_SRC` for the session's komorebi, `$KOMOREBI_SOURCE`, or this build's]
    #[arg(long = "komorebi-src", alias = "code-root", value_name = "DIR")]
    pub code_root: Option<PathBuf>,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

/// The arguments of `annotate delete`.
#[derive(Args, Debug)]
pub struct DeleteArgs {
    /// The annotation id, e.g. a-3f9c1e02
    pub id: String,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

/// The arguments of `annotations`.
#[derive(Args, Debug)]
pub struct AnnotationsArgs {
    /// Session directory
    pub session: PathBuf,
    /// Only this kind
    #[arg(long, value_enum)]
    pub kind: Option<KindArg>,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

impl AnnotateArgs {
    /// Returns `true` if and only if the action was asked for `--json`, which also decides how a
    /// failure is reported.
    #[must_use]
    pub fn json(&self) -> bool {
        match &self.action {
            Action::Add(args) => args.json,
            Action::Edit(args) => args.json,
            Action::Delete(args) => args.json,
        }
    }
}

/// The `schema` of the output of `annotate add` and `annotate edit` with `--json`: the annotation as
/// it reads after the change ([`AnnotationData`]).
pub const SCHEMA_ANNOTATION: &str = "annotation/1";
/// The `schema` of the output of `annotations` with `--json` ([`AnnotationsData`]).
pub const SCHEMA_ANNOTATIONS: &str = "annotations/1";
/// The `schema` of the output of `annotate delete` with `--json`: the id removed ([`DeletedData`]).
pub const SCHEMA_DELETED: &str = "annotation-deleted/1";

/// Reads the references, finding the log line of one that names it by text or time
/// ([`super::refspec`]). The notes say what each of those was resolved to.
fn parse_refs(
    session: &Session,
    manifest: &Manifest,
    refs: &[String],
) -> eyre::Result<(Vec<Ref>, Vec<String>)> {
    let mut parsed = Vec::new();
    let mut notes = Vec::new();
    for text in refs {
        let resolved = super::refspec::resolve(session, manifest, text)?;
        parsed.push(resolved.reference.parse()?);
        notes.extend(resolved.note);
    }
    Ok((parsed, notes))
}

fn resolve_ns(session: &Session, manifest: &Manifest, text: &str) -> eyre::Result<u64> {
    let spec: TimeSpec = text.parse()?;
    Ok(spec
        .resolve(manifest, &SessionLookup::new(session, manifest))?
        .mach_ns)
}

fn emit(
    json: bool,
    schema: &str,
    annotation: &annotations::Annotation,
    manifest: &Manifest,
    warnings: Vec<String>,
) -> eyre::Result<()> {
    let view = annotation.view(manifest);
    if json {
        return print_json(
            &Envelope::ok(schema, AnnotationData { annotation: view }).warnings(warnings),
        );
    }

    print!("{}", report::annotation_text(&view));
    for warning in warnings {
        eprintln!("note: {warning}");
    }
    Ok(())
}

pub(super) fn annotate(args: &AnnotateArgs) -> eyre::Result<()> {
    // not `load`: marking has to work on a session that is still being recorded
    let session = Session::open(&args.session)?;
    let manifest = session.manifest()?;

    match &args.action {
        Action::Add(add) => add_annotation(&session, &manifest, add),
        Action::Edit(edit) => edit_annotation(&session, &manifest, edit),
        Action::Delete(delete) => {
            annotations::delete(&session.dir, &delete.id)?;
            if delete.json {
                return print_json(&Envelope::ok(
                    SCHEMA_DELETED,
                    DeletedData {
                        deleted: delete.id.clone(),
                    },
                ));
            }
            println!("deleted {}", delete.id);
            Ok(())
        }
    }
}

fn add_annotation(session: &Session, manifest: &Manifest, args: &AddArgs) -> eyre::Result<()> {
    let kind = Kind::from(args.kind);
    let loaded = annotations::load(&session.dir)?;
    let (refs, resolved) = parse_refs(session, manifest, &args.refs)?;
    let about = match &args.about {
        Some(id) => Some(
            loaded
                .get(id)
                .ok_or_else(|| envelope::not_found(format!("there is no annotation {id}")))?,
        ),
        None => None,
    };

    let text = args.text.clone().unwrap_or_default();
    if text.trim().is_empty() {
        return Err(envelope::usage(match kind {
            Kind::Bug => "a bug needs --text: what does it look like?",
            Kind::Note => "a note needs --text",
            Kind::Finding => "a finding needs --text: what is the cause?",
        }));
    }

    if kind == Kind::Finding && refs.is_empty() {
        return Err(envelope::usage(
            "a finding needs evidence: pass at least one --ref (event:<seq>, log:<line>, \
             code:<path>:<line> or frame:<time>)",
        ));
    }

    if kind == Kind::Bug && args.end.is_none() && !args.open_ended {
        return Err(envelope::usage(
            "a bug region needs --end (or --open-ended if it was still happening when the \
             recording stopped)",
        ));
    }

    // a finding without a time of its own takes the bug it explains, else its earliest evidence
    let start = if let Some(start) = &args.start {
        resolve_ns(session, manifest, start)?
    } else if let Some(about) = about {
        about.start_mach_ns
    } else if kind == Kind::Finding {
        let lookup = SessionLookup::new(session, manifest);
        let earliest = refs
            .iter()
            .filter_map(|reference| match reference {
                Ref::Event { seq } => format!("event:{seq}").parse::<TimeSpec>().ok(),
                Ref::Frame { video_s } => format!("{video_s}").parse::<TimeSpec>().ok(),
                _ => None,
            })
            .filter_map(|spec| spec.resolve(manifest, &lookup).ok())
            .map(|resolved| resolved.mach_ns)
            .min();
        earliest.ok_or_else(|| {
            envelope::usage("say when with --start or --about <bug id>: no evidence has a time")
        })?
    } else {
        return Err(envelope::usage("--start is required"));
    };

    let end = match (&args.end, about, kind) {
        (Some(end), _, _) => Some(resolve_ns(session, manifest, end)?),
        (None, Some(about), Kind::Finding) => about.end_mach_ns,
        _ => None,
    };

    let code_root = args
        .code_root
        .clone()
        .or_else(|| annotations::default_code_root(manifest.komorebi.flavor));
    let mut warnings = resolved;
    warnings.extend(annotations::validate_refs(
        session,
        manifest,
        &refs,
        code_root.as_deref(),
    )?);

    let author = args.author.clone().unwrap_or_else(|| {
        if kind == Kind::Finding {
            "agent".to_string()
        } else {
            annotations::AUTHOR_USER.to_string()
        }
    });

    let id = annotations::add(
        &session.dir,
        New {
            kind: Some(kind),
            start_mach_ns: start,
            end_mach_ns: end,
            title: args.title.clone().unwrap_or_default(),
            text,
            expected: args.expected.clone(),
            displays: args.displays.clone(),
            author: Some(author),
            about: args.about.clone(),
            refs,
        },
    )?;

    let added = annotations::load(&session.dir)?;
    let annotation = added.get(&id).ok_or_else(|| {
        eyre::Report::msg("the annotation was written but could not be read back")
    })?;
    emit(args.json, SCHEMA_ANNOTATION, annotation, manifest, warnings)
}

fn edit_annotation(session: &Session, manifest: &Manifest, args: &EditArgs) -> eyre::Result<()> {
    let (refs, mut warnings) = if args.refs.is_empty() {
        (None, Vec::new())
    } else {
        let (refs, notes) = parse_refs(session, manifest, &args.refs)?;
        (Some(refs), notes)
    };
    if let Some(refs) = &refs {
        let code_root = args
            .code_root
            .clone()
            .or_else(|| annotations::default_code_root(manifest.komorebi.flavor));
        warnings.extend(annotations::validate_refs(
            session,
            manifest,
            refs,
            code_root.as_deref(),
        )?);
    }

    let patch = Patch {
        start_mach_ns: args
            .start
            .as_deref()
            .map(|text| resolve_ns(session, manifest, text))
            .transpose()?,
        end_mach_ns: args
            .end
            .as_deref()
            .map(|text| resolve_ns(session, manifest, text))
            .transpose()?,
        title: args.title.clone(),
        text: args.text.clone(),
        expected: args.expected.clone(),
        displays: (!args.displays.is_empty()).then(|| args.displays.clone()),
        refs,
    };

    annotations::edit(&session.dir, &args.id, patch)?;
    let loaded = annotations::load(&session.dir)?;
    let annotation = loaded.get(&args.id).ok_or_else(|| {
        eyre::Report::msg("the annotation was written but could not be read back")
    })?;
    emit(args.json, SCHEMA_ANNOTATION, annotation, manifest, warnings)
}

pub(super) fn list(args: &AnnotationsArgs) -> eyre::Result<()> {
    let session = Session::open(&args.session)?;
    let manifest = session.manifest()?;
    let loaded = annotations::load(&session.dir)?;
    let wanted = args.kind.map(Kind::from);

    let views: Vec<AnnotationView> = loaded
        .annotations
        .iter()
        .filter(|annotation| wanted.is_none_or(|kind| annotation.kind == kind))
        .map(|annotation| annotation.view(&manifest))
        .collect();

    if args.json {
        return print_json(
            &Envelope::ok(SCHEMA_ANNOTATIONS, AnnotationsData { annotations: views })
                .warnings(loaded.warnings),
        );
    }

    if views.is_empty() {
        println!("no annotations; add one with `komorebi-recorder annotate <session> add ...`");
    }
    for view in &views {
        print!("{}", report::annotation_text(view));
    }
    for warning in &loaded.warnings {
        eprintln!("warning: {warning}");
    }
    Ok(())
}
