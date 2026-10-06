//! The `sessions` binary. All behavior lives in the library crate.

fn main() -> anyhow::Result<()> {
    sessions::run()
}
