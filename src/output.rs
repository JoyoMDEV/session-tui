//! Standard output that survives a closed pipe. `println!` panics when the reader has gone away, as
//! in `sessions prune | head`, so everything the program prints goes through here, and clippy
//! denies the macros (see `lib.rs`). A closed pipe means the reader has all it wants, so it is not
//! an error: the functions only report it, by returning `false`, and a loop can stop.

use std::fmt::Arguments;
use std::io::Write;

/// Writes the arguments and a newline to stdout. `false` once nothing can be written any more.
pub fn line(args: Arguments<'_>) -> bool {
    writeln!(std::io::stdout().lock(), "{args}").is_ok()
}

/// Writes `text` as it is, without a newline. `false` once nothing can be written any more.
pub fn text(text: &str) -> bool {
    write!(std::io::stdout().lock(), "{text}").is_ok()
}

/// `println!` for stdout that ends quietly on a closed pipe. Evaluates to `false` when it did.
macro_rules! say {
    ($($arg:tt)*) => {
        $crate::output::line(format_args!($($arg)*))
    };
}
pub(crate) use say;
