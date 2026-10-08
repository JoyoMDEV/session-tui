//! The `sessions` program as a library. `main.rs` only calls [`run`]; this file parses the command
//! line and hands each command to the module that implements it. Output that may be piped goes
//! through `writeln!`, never `println!`.

mod cli;
mod commands;
mod doctor;
mod hook;
mod launch;
mod query;
mod setup;
mod store;
mod tags;
mod tickets;
mod transcript;
mod tui;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Cmd};
use commands::{edit, import, list, log, migrate_tickets, prune, resume, rm};

/// Parses the command line and runs the command; with none given, starts the browser.
pub fn run() -> Result<()> {
    match Cli::parse().cmd {
        None => tui::run(),
        Some(Cmd::Setup { force }) => setup::run(force),
        Some(Cmd::Doctor) => doctor::run(),
        Some(Cmd::Hook) => hook::run(),
        Some(Cmd::Title { id, title }) => edit::title(id, title),
        Some(Cmd::Ticket { id, remove, keys }) => edit::ticket(id, remove, keys),
        Some(Cmd::MigrateTickets { yes }) => migrate_tickets::run(yes),
        Some(Cmd::Tag { id, remove, names }) => edit::tag(id, remove, names),
        Some(Cmd::Note { id, text }) => edit::note(id, text),
        Some(Cmd::Import) => import::run(),
        Some(Cmd::Prune { yes }) => prune::run(yes),
        Some(Cmd::Rm { id }) => rm::run(id),
        Some(Cmd::List { filters, json }) => list::run(filters, json),
        Some(Cmd::Log { filters, markdown }) => log::run(filters, markdown),
        Some(Cmd::Resume {
            query,
            filters,
            list,
        }) => resume::run(query, filters, list),
    }
}
