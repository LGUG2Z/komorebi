use clap::Parser;
use schemars_nixgen::GeneratorOptions;
use schemars_nixgen::generate_from_schema;
use std::fs;
use std::io::Write;
use std::io::{self};
use std::path::PathBuf;

/// Generate Nix option definitions from JSON Schema
#[derive(Parser, Debug)]
#[command(name = "schemars-nixgen")]
#[command(about = "Generate Nix option definitions from JSON Schema")]
struct Cli {
    /// Input JSON Schema file
    input: PathBuf,

    /// Output Nix file (default: stdout)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Root option name (e.g., "komorebi" or "komorebi-bar")
    #[arg(short, long, default_value = "config")]
    name: String,

    /// Description for the root option
    #[arg(short, long, default_value = "Configuration options")]
    description: String,
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let cli = Cli::parse();

    // Read the input schema
    let schema_str = fs::read_to_string(&cli.input)?;
    let schema: serde_json::Value = serde_json::from_str(&schema_str)?;

    // Configure generator options
    let options = GeneratorOptions {
        root_name: cli.name,
        root_description: cli.description,
    };

    // Generate Nix code
    let nix_code = generate_from_schema(&schema, &options)?;

    // Write output
    if let Some(output_path) = cli.output {
        fs::write(&output_path, &nix_code)?;
        eprintln!(
            "Generated Nix options written to: {}",
            output_path.display()
        );
    } else {
        io::stdout().write_all(nix_code.as_bytes())?;
    }

    Ok(())
}
