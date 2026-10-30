//! Compile Spatial programs (`.spt`) for the PX8 spatial protocol chip.
//!
//! A program wires the chip's hardware tiles (pins, timers, shifters,
//! counters, logic cells) into a protocol, exports streams the host can use,
//! and carries its own tests:
//!
//! ```
//! let build = spatial::compile(r#"
//!     pin led: out = blink.out
//!     timer blink { every: 1 us, run: 1 }
//! "#)?;
//! assert_eq!(build.pins["led"], 0);
//! assert!(build.image.len() <= 1024);
//! # Ok::<(), spatial::Error>(())
//! ```
//!
//! Everything is checked against what the chip actually has: a program that
//! needs a fifth timer, a ninth pin, or a duration that is not a whole number
//! of clock cycles is rejected with an [`Error`] pointing at the source.

mod byte_pipeline;
mod control;
mod diagnostic;
mod elaborate;
mod hardware;
mod logic;
mod plan;
mod px8;
mod syntax;
mod value;

pub use diagnostic::{Error, Span};
use std::collections::BTreeMap;

/// The spatial RTL revision whose register layout this compiler targets.
pub const HARDWARE_REVISION: &str = "58da19f41c4fc91d68bd052c0105c45efbcef8ee";

/// A compiled program.
#[derive(Debug)]
pub struct Build {
    /// SRAM image: controller code followed by configuration tables.
    pub image: Vec<u8>,
    /// Physical `uio` pad assigned to each pin.
    pub pins: BTreeMap<String, usize>,
    /// Hardware used, as `(name, used, available)`; `memory` is in bytes.
    pub usage: Vec<(&'static str, usize, usize)>,
    /// Names of the program's tests, in source order.
    pub tests: Vec<String>,
    /// Everything the simulator needs to run the tests, as JSON.
    pub plan: serde_json::Value,
}

/// Parse, check and compile a program.
///
/// # Errors
/// Returns the first problem found, with the source range it concerns.
pub fn compile(source: &str) -> Result<Build, Error> {
    let items = syntax::parse(source)?;
    let design = elaborate::elaborate(&items)?;
    let plan = plan::plan(&design, source)?;
    Ok(Build {
        tests: design
            .tests
            .iter()
            .map(|(name, _)| name.text.clone())
            .collect(),
        usage: design.usage,
        pins: design.pins,
        image: design.image,
        plan,
    })
}
