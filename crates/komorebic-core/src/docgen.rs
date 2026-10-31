//! The pages of `docs/cli`: the long help of every subcommand, one Markdown file each.

use clap::Command;
use std::io;
use std::path::Path;
use std::path::PathBuf;

/// The subcommands that get no page: the generator itself, and the ones that are hidden or
/// platform plumbing that is not documented.
pub const IGNORED: [&str; 12] = [
    "docgen",
    "splash",
    "alt-focus-hack",
    "identify-border-overflow-application",
    "load-custom-layout",
    "workspace-custom-layout",
    "named-workspace-custom-layout",
    "workspace-custom-layout-rule",
    "named-workspace-custom-layout-rule",
    "focus-follows-mouse",
    "toggle-focus-follows-mouse",
    "format-app-specific-configuration",
];

/// How a platform's pages look. Each tree keeps the format its checked-in pages have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocFormat {
    /// Wrap the help at this many columns whatever the width of the terminal, or at the width of
    /// the terminal if there is none.
    pub term_width: Option<usize>,
    /// What the opening fence of a page says (`text`), or nothing.
    pub fence_info: &'static str,
    /// Whether a page ends with a newline after the closing fence.
    pub trailing_newline: bool,
    /// Whether a subcommand of a subcommand (`hotkeys compile`) gets a page of its own, named
    /// `hotkeys-compile`.
    pub nested: bool,
}

impl DocFormat {
    /// The pages of komorebic for Windows and for macOS.
    pub const PLAIN: Self = Self {
        term_width: None,
        fence_info: "",
        trailing_newline: false,
        nested: false,
    };

    /// The pages of komorebic for KDE: the same whatever the width of the terminal.
    pub const KDE: Self = Self {
        term_width: Some(100),
        fence_info: "text",
        trailing_newline: true,
        nested: true,
    };

    fn render(&self, cmd: Command) -> String {
        let mut cmd = match self.term_width {
            Some(width) => cmd.term_width(width),
            None => cmd,
        };
        let help_text = cmd.render_long_help().to_string();
        let newline = if self.trailing_newline { "\n" } else { "" };
        format!("```{}\n{help_text}\n```{newline}", self.fence_info)
    }
}

/// The pages of a command line, as `(file name, content)` in the order of its subcommands.
pub fn pages(mut cli: Command, format: &DocFormat) -> Vec<(String, String)> {
    let mut pages = Vec::new();
    for cmd in cli.get_subcommands_mut() {
        let name = cmd.get_name().to_string();
        if IGNORED.contains(&name.as_str()) {
            continue;
        }
        pages.push((format!("{name}.md"), format.render(cmd.clone())));
        if format.nested {
            for sub in cmd.get_subcommands() {
                pages.push((
                    format!("{name}-{}.md", sub.get_name()),
                    format.render(sub.clone()),
                ));
            }
        }
    }
    pages
}

/// Write the pages into `dir` (made if it is not there) and return the paths written.
pub fn write(cli: Command, format: &DocFormat, dir: &Path) -> io::Result<Vec<PathBuf>> {
    std::fs::create_dir_all(dir)?;
    pages(cli, format)
        .into_iter()
        .map(|(file, content)| {
            let path = dir.join(file);
            std::fs::write(&path, content)?;
            Ok(path)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    use clap::Parser;
    use clap::Subcommand;

    #[derive(Parser)]
    struct Cli {
        #[command(subcommand)]
        cmd: Cmd,
    }

    #[derive(Subcommand)]
    enum Cmd {
        /// Do the thing
        Thing,
        /// Group of things
        Group {
            #[command(subcommand)]
            sub: Sub,
        },
        #[command(hide = true)]
        Docgen,
    }

    #[derive(Subcommand)]
    enum Sub {
        /// Do one of the things
        One,
    }

    #[test]
    fn a_plain_page_has_a_bare_fence_and_no_final_newline() {
        let pages = pages(Cli::command(), &DocFormat::PLAIN);
        let names: Vec<_> = pages.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["thing.md", "group.md"]);
        assert!(pages[0].1.starts_with("```\nDo the thing\n"));
        assert!(pages[0].1.ends_with("\n```"));
    }

    #[test]
    fn a_kde_page_says_text_ends_in_a_newline_and_nests() {
        let pages = pages(Cli::command(), &DocFormat::KDE);
        let names: Vec<_> = pages.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["thing.md", "group.md", "group-one.md"]);
        assert!(pages[0].1.starts_with("```text\nDo the thing\n"));
        assert!(pages[0].1.ends_with("\n```\n"));
    }

    #[test]
    fn write_returns_the_paths_it_wrote() {
        let dir =
            std::env::temp_dir().join(format!("komorebic-core-docgen-{}", std::process::id()));
        let written = write(Cli::command(), &DocFormat::PLAIN, &dir).unwrap();
        assert_eq!(written.len(), 2);
        assert!(
            std::fs::read_to_string(dir.join("thing.md"))
                .unwrap()
                .contains("Do the thing")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
