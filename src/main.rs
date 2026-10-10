//! The `sessions` binary. All behavior lives in the library crate.

use std::process::ExitCode;

fn main() -> anyhow::Result<ExitCode> {
    sessions::run()
}
