#[expect(
    clippy::unreadable_literal,
    reason = "RISC Zero generates image IDs as decimal words without separators"
)]
mod generated {
    include!(concat!(env!("OUT_DIR"), "/methods.rs"));
}

pub use generated::*;
