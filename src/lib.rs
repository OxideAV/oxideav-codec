//! Codec traits and registry — re-export shim.
//!
//! Historically this crate hosted the `Decoder` / `Encoder` traits and
//! the `CodecRegistry`. Those types moved to `oxideav-core` so the
//! unified `RuntimeContext` (which holds the codec registry alongside
//! the container / source / filter registries) doesn't pull a circular
//! dependency. This crate is now a thin re-export so existing
//! `use oxideav_codec::Decoder;` continues to compile unchanged.

pub use oxideav_core::{
    CodecImplementation, CodecInfo, CodecRegistry, Decoder, DecoderFactory, Encoder,
    EncoderFactory,
};

/// Compatibility module path for callers that imported through
/// `oxideav_codec::registry::*`. The relocated types live in
/// [`oxideav_core::registry::codec`].
pub mod registry {
    pub use oxideav_core::registry::codec::{
        CodecImplementation, CodecInfo, CodecRegistry, Decoder, DecoderFactory, Encoder,
        EncoderFactory,
    };
}
