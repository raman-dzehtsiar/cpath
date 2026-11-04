// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

use std::process::ExitCode;

use cpath::cli::{self, Command};

fn main() -> ExitCode {
    let version = env!("CARGO_PKG_VERSION");
    let command = match cli::parse(std::env::args_os().skip(1)) {
        Ok(command) => command,
        Err(error) => {
            let _ = cpath::report::write_error_json(std::io::stdout().lock(), version, 2, &error);
            return ExitCode::from(2);
        }
    };
    match command {
        Command::Gate(arguments) => return ExitCode::from(cpath::gate::run(&arguments)),
        Command::Help => println!("{}", cli::HELP),
        Command::Version => println!("{}", env!("CARGO_PKG_VERSION")),
        Command::Scan {
            root,
            options,
            pretty,
        } => {
            let report = match cpath::pipeline::scan(&root, &options) {
                Ok(report) => report,
                Err(error) => {
                    let _ = cpath::report::write_error_json(
                        std::io::stdout().lock(),
                        version,
                        1,
                        &error,
                    );
                    return ExitCode::FAILURE;
                }
            };
            let exit_code = if report.unscored_files > 0 { 3 } else { 0 };
            if cpath::report::write_json(
                std::io::stdout().lock(),
                version,
                &report,
                exit_code,
                pretty,
            )
            .is_err()
            {
                return ExitCode::FAILURE;
            }
            return ExitCode::from(exit_code);
        }
    }
    ExitCode::SUCCESS
}
