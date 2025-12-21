use clap::Parser;
use schemars_docgen::Options;
use schemars_docgen::generate_html;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "schemars-docgen")]
#[command(about = "Generate HTML documentation from JSON Schema")]
struct Cli {
    /// Input JSON Schema file
    input: PathBuf,

    /// Output HTML file
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Schema title (overrides schema title from the schema)
    #[arg(short, long)]
    title: Option<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let schema_json = std::fs::read_to_string(&cli.input)?;
    let schema: schemars::Schema = serde_json::from_str(&schema_json)?;

    let options = Options {
        title_override: cli.title,
    };

    let html = generate_html(&schema, &options)?;

    let output_path = cli.output.unwrap_or_else(|| {
        let mut path = cli.input.clone();
        path.set_extension("html");
        path
    });

    std::fs::write(&output_path, html)?;
    println!("Generated documentation: {}", output_path.display());

    Ok(())
}
