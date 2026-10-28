//! Compile named hardware workers and connections into a PX8 startup image.
//!
//! The input language is TOML, not Rust. The compiler assigns finite resources,
//! validates their wiring, then emits configuration writes followed by an idle
//! loop. It does not prove protocol timing, FIFO service rates or electrical safety.
//!
//! ```
//! let result = spatial_compiler::compile(r#"
//! [[timers]]
//! name = "beat"
//! period_cycles = 64
//! [[pins]]
//! name = "wire"
//! mode = "push_pull"
//! source = "beat.out"
//! "#)?;
//! assert_eq!(result.report.timers["beat"], 0);
//! assert!(result.image.len() <= 1024);
//! # Ok::<(), spatial_compiler::CompileError>(())
//! ```

mod backend;
mod constraints;
mod frontend;

use serde::Serialize;
use std::{collections::BTreeMap, fmt};

/// Hardware revision whose register layout and limits this compiler targets.
pub const HARDWARE_REVISION: &str = "58da19f41c4fc91d68bd052c0105c45efbcef8ee";
/// Configuration/bytecode ABI; changing hardware layouts requires a new identity.
pub const ABI: &str = "px8-spatial-v1";

/// A rejected source program, with a stable category and actionable explanation.
#[derive(Debug)]
pub struct CompileError {
    /// Constraint category, for example `pin-capacity` or `signal-class`.
    pub code: &'static str,
    /// The resource and requirement that failed.
    pub message: String,
}
impl CompileError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}
impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for CompileError {}

/// Resource assignments and encoding evidence accompanying the emitted bytes.
#[derive(Debug, Serialize)]
pub struct Report {
    /// Fabric ABI identity.
    pub abi: &'static str,
    /// Exact RTL revision, not a moving branch name.
    pub hardware_revision: &'static str,
    /// Assumed chip clock; not a physical timing-signoff claim.
    pub clock_hz: u64,
    /// Logical pin names to distinct physical uio pads.
    pub pins: BTreeMap<String, usize>,
    /// Named timers to hardware timer indices.
    pub timers: BTreeMap<String, usize>,
    /// Named shifters to hardware indices, also their FIFO channels.
    pub shifter_fifo_channels: BTreeMap<String, usize>,
    /// Pin electrical modes, used to validate simulation wiring.
    pub pin_modes: BTreeMap<String, String>,
    /// Shifters configured to consume the host TX FIFO.
    pub tx_shifters: Vec<String>,
    /// Shifters configured to produce the host RX FIFO.
    pub rx_shifters: Vec<String>,
    /// Actual code + table + data length.
    pub image_bytes: usize,
    /// SRAM capacity shared by all image sections.
    pub capacity_bytes: usize,
    /// Offset of opaque data; data is not automatically transmitted.
    pub data_offset: usize,
    /// Ordered register writes, ending with the arm operation.
    pub config: Vec<(u8, u16)>,
}

/// A successful compilation. Reset hardware before loading the image.
#[derive(Debug)]
pub struct Compilation {
    /// Complete SRAM image, at most 1024 bytes.
    pub image: Vec<u8>,
    /// Assignments, configuration writes and target identity.
    pub report: Report,
}

/// Parse, validate, allocate and encode a source program without filesystem I/O.
///
/// There is deliberately no public unchecked AST-to-bytecode entry point.
///
/// # Errors
/// Returns a diagnostic for invalid TOML, unsupported fields, impossible resource
/// assignments, incompatible signals, out-of-range values or an oversized image.
pub fn compile(source: &str) -> Result<Compilation, CompileError> {
    backend::compile(frontend::parse(source)?)
}
