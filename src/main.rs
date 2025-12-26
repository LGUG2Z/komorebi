#![warn(clippy::all)]

use clap::Parser;
use clap::ValueEnum;
use schemars_docgen::MdxOptions;
use schemars_docgen::Options;
use schemars_docgen::generate_html;
use schemars_docgen::generate_mdx;
use std::path::PathBuf;

/// Output format for documentation generation
#[derive(Debug, Clone, Copy, Default, ValueEnum)]
enum Format {
    /// HTML output (self-contained single file)
    #[default]
    Html,
    /// MDX output for Astro Starlight
    Mdx,
}

#[derive(Parser)]
#[command(name = "schemars-docgen")]
#[command(about = "Generate documentation from JSON Schema")]
struct Cli {
    /// Input JSON Schema file
    input: PathBuf,

    /// Output file (defaults to input filename with appropriate extension)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Output format
    #[arg(short, long, default_value = "html")]
    format: Format,

    /// Schema title (overrides title from the schema)
    #[arg(short, long)]
    title: Option<String>,

    /// Schema description (overrides description from the schema, MDX only)
    #[arg(short, long)]
    description: Option<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let schema_json = std::fs::read_to_string(&cli.input)?;
    let schema: schemars::Schema = serde_json::from_str(&schema_json)?;

    let (output, extension) = match cli.format {
        Format::Html => {
            let options = Options {
                title_override: cli.title,
            };
            (generate_html(&schema, &options)?, "html")
        }
        Format::Mdx => {
            let options = MdxOptions {
                title_override: cli.title,
                description_override: cli.description,
            };
            (generate_mdx(&schema, &options)?, "mdx")
        }
    };

    let output_path = cli.output.unwrap_or_else(|| {
        let mut path = cli.input.clone();
        path.set_extension(extension);
        path
    });

    std::fs::write(&output_path, output)?;
    println!("Generated documentation: {}", output_path.display());

    Ok(())
}
