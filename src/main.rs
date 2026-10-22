// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

use std::io;
use std::process::ExitCode;

use cpath::cli::{self, Command};

fn main() -> ExitCode {
    let command = match cli::parse(std::env::args_os().skip(1)) {
        Ok(command) => command,
        Err(error) => {
            eprintln!("cpath: {error}. Run 'cpath --help' for available options.");
            return ExitCode::from(2);
        }
    };
    match command {
        Command::Help => println!("{}", cli::HELP),
        Command::Version => println!("cpath {}", env!("CARGO_PKG_VERSION")),
        Command::Scan { root, top, options } => {
            let report = match cpath::pipeline::scan(&root, &options) {
                Ok(report) => report,
                Err(error) => {
                    eprintln!("cpath: {error}");
                    return ExitCode::FAILURE;
                }
            };
            if let Err(error) = cpath::report::write_diagnostics(io::stderr().lock(), &report) {
                eprintln!("cpath: cannot write coverage diagnostics: {error}");
                return ExitCode::FAILURE;
            }
            if let Err(error) =
                cpath::report::write_top(io::stdout().lock(), &report.root, &report.ranked, top)
            {
                if error.kind() != io::ErrorKind::BrokenPipe {
                    eprintln!("cpath: cannot write results: {error}");
                    return ExitCode::FAILURE;
                }
            }
            if report.unscored_files > 0 {
                return ExitCode::from(3);
            }
        }
    }
    ExitCode::SUCCESS
}
