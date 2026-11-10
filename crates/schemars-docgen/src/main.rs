#![warn(clippy::all)]

use clap::Parser;
use schemars_docgen::MdxOptions;
use schemars_docgen::generate_mdx;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "schemars-docgen")]
#[command(about = "Generate documentation from JSON Schema")]
struct Cli {
    /// Input JSON Schema file
    input: PathBuf,

    /// Output file (defaults to input filename with .mdx extension)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Schema title (overrides title from the schema)
    #[arg(short, long)]
    title: Option<String>,

    /// Schema description (overrides description from the schema)
    #[arg(short, long)]
    description: Option<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let schema_json = std::fs::read_to_string(&cli.input)?;
    let schema: schemars::Schema = serde_json::from_str(&schema_json)?;

    let options = MdxOptions {
        title_override: cli.title,
        description_override: cli.description,
    };
    let output = generate_mdx(&schema, &options)?;

    let output_path = cli.output.unwrap_or_else(|| {
        let mut path = cli.input.clone();
        path.set_extension("mdx");
        path
    });

    std::fs::write(&output_path, output)?;
    println!("Generated documentation: {}", output_path.display());

    Ok(())
}
