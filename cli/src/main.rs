use std::collections::HashSet;
use std::path::PathBuf;
use std::process::ExitCode;
use clap::{CommandFactory, Parser};

use program_analysis::config;
use program_analysis::analysis_runner::AnalysisRunner;

use program_structure::constants::Curve;
use program_structure::file_definition::FileID;
use program_structure::report::Report;
use program_structure::report::MessageCategory;
use program_structure::writers::{LogWriter, ReportWriter, SarifWriter, CachedStdoutWriter};

#[derive(Parser, Debug)]
#[command(styles=cli_styles())]
/// A static analyzer and linter for Circom programs.
struct Cli {
    /// Initial input file(s)
    #[clap(name = "INPUT")]
    input_files: Vec<PathBuf>,

    /// Library file paths
    #[clap(short = 'L', long = "library", name = "LIBRARIES")]
    libraries: Vec<PathBuf>,

    /// Output level (INFO, WARNING, or ERROR)
    #[clap(short = 'l', long = "level", name = "LEVEL", default_value = config::DEFAULT_LEVEL)]
    output_level: MessageCategory,

    /// Output analysis results to a Sarif file
    #[clap(short, long, name = "OUTPUT")]
    sarif_file: Option<PathBuf>,

    /// Ignore results from given analysis passes
    #[clap(short = 'a', long = "allow", name = "ID")]
    allow_list: Vec<String>,

    /// Enable verbose output
    #[clap(short = 'v', long = "verbose")]
    verbose: bool,

    /// Set curve (BN254, BLS12_381, or GOLDILOCKS)
    #[clap(short = 'c', long = "curve", name = "NAME", default_value = config::DEFAULT_CURVE)]
    curve: Curve,
}

/// Styles the help output for the [`Cli`].
fn cli_styles() -> clap::builder::Styles {
    use clap::builder::styling::*;

    Styles::styled()
        .header(AnsiColor::Yellow.on_default())
        .usage(AnsiColor::Green.on_default())
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Green.on_default())
}

/// Returns true if a primary location of the report corresponds to a file
/// specified on the command line by the user.
fn filter_by_file(report: &Report, user_inputs: &HashSet<FileID>) -> bool {
    report.primary_file_ids().iter().any(|file_id| user_inputs.contains(file_id))
}

/// Returns true if the report level is greater than or equal to the given
/// level.
fn filter_by_level(report: &Report, output_level: &MessageCategory) -> bool {
    report.category() >= output_level
}

/// Returns true if the report ID is not in the given list.
fn filter_by_id(report: &Report, allow_list: &[String]) -> bool {
    !allow_list.contains(&report.id())
}

fn main() -> ExitCode {
    // Initialize logger and options.
    pretty_env_logger::init();
    let options = Cli::parse();
    if options.input_files.is_empty() {
        match Cli::command().print_help() {
            Ok(()) => return ExitCode::SUCCESS,
            Err(_) => return ExitCode::FAILURE,
        }
    }

    // Set up analysis runner.
    let (mut runner, reports) = AnalysisRunner::new(options.curve)
        .with_libraries(&options.libraries)
        .with_files(&options.input_files);

    // Set up writer and write reports to `stdout`.
    let allow_list = options.allow_list.clone();
    let user_inputs = runner.file_library().user_inputs().clone();
    let mut stdout_writer = CachedStdoutWriter::new(options.verbose)
        .add_filter(move |report: &Report| filter_by_level(report, &options.output_level))
        .add_filter(move |report: &Report| filter_by_file(report, &user_inputs))
        .add_filter(move |report: &Report| filter_by_id(report, &allow_list));
    stdout_writer.write_reports(&reports, runner.file_library());

    // Analyze functions and templates in user provided input files.
    runner.analyze_functions(&mut stdout_writer, true);
    runner.analyze_templates(&mut stdout_writer, true);

    // If a Sarif file is passed to the program we write the reports to it.
    if let Some(sarif_file) = options.sarif_file {
        let allow_list = options.allow_list.clone();
        let user_inputs = runner.file_library().user_inputs().clone();
        let mut sarif_writer = SarifWriter::new(&sarif_file)
            .add_filter(move |report: &Report| filter_by_level(report, &options.output_level))
            .add_filter(move |report: &Report| filter_by_file(report, &user_inputs))
            .add_filter(move |report: &Report| filter_by_id(report, &allow_list));
        if sarif_writer.write_reports(stdout_writer.reports(), runner.file_library()) > 0 {
            stdout_writer.write_message(&format!("Result written to `{}`.", sarif_file.display()));
        }
    }

    // Use the exit code to indicate if any issues were found.
    match stdout_writer.reports_written() {
        0 => {
            stdout_writer.write_message("No issues found.");
            ExitCode::SUCCESS
        }
        1 => {
            stdout_writer.write_message("1 issue found.");
            ExitCode::FAILURE
        }
        n => {
            stdout_writer.write_message(&format!("{n} issues found."));
            ExitCode::FAILURE
        }
    }
}
