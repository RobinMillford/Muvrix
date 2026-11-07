//! Background media scanner, recursive crawler, and incremental change detector.

pub mod engine;
pub mod options;

pub use engine::{ScanSummary, ScannerService};
pub use options::{ScanOptions, ScanProgress};
