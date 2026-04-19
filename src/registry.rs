//! In-process codec registry — supports multiple implementations per codec
//! id, ranked by capability + priority + user preferences with init-time
//! fallback.

use std::collections::HashMap;

use oxideav_core::{
    CodecCapabilities, CodecId, CodecParameters, CodecPreferences, CodecResolver, CodecTag, Error,
    Result,
};

use crate::{Decoder, DecoderFactory, Encoder, EncoderFactory};

/// A bitstream-probe function that inspects the first bytes of a packet
/// and returns true if this codec can decode it. Lets the registry
/// disambiguate the many container tags that get mislabelled in the
/// wild (most famous: AVI FourCC `DIV3` routing to MPEG-4 Part 2
/// instead of MS-MPEG4v3).
pub type CodecProbe = fn(&[u8]) -> bool;

/// One codec's claim on a container tag. Stored inside the registry;
/// callers don't usually construct this directly, see
/// [`CodecRegistry::claim_tag`].
#[derive(Clone, Copy)]
pub struct TagClaim {
    /// Higher = preferred when multiple codecs claim the same tag.
    pub priority: u8,
    /// Optional bitstream probe. Returns true to accept this claim,
    /// false to skip and try the next one in priority order.
    pub probe: Option<CodecProbe>,
}

/// One registered implementation: capability description + factories.
/// Either / both factories may be present depending on whether the impl
/// can decode, encode, or both.
#[derive(Clone)]
pub struct CodecImplementation {
    pub caps: CodecCapabilities,
    pub make_decoder: Option<DecoderFactory>,
    pub make_encoder: Option<EncoderFactory>,
}

#[derive(Default)]
pub struct CodecRegistry {
    impls: HashMap<CodecId, Vec<CodecImplementation>>,
    /// Tag-to-codec-id map. Containers call [`Self::resolve_tag`] to
    /// turn their in-stream codec tag (FourCC / WAVEFORMATEX /
    /// Matroska id / …) into a [`CodecId`] without hard-coding the
    /// mapping themselves. Claims are kept sorted by priority
    /// descending so resolution is cheap.
    tag_claims: HashMap<CodecTag, Vec<(CodecId, TagClaim)>>,
}

impl CodecRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a codec implementation. The same codec id may be registered
    /// multiple times — for example a software FLAC decoder and (later) a
    /// hardware one would both register under id `"flac"`.
    pub fn register(&mut self, id: CodecId, implementation: CodecImplementation) {
        self.impls.entry(id).or_default().push(implementation);
    }

    /// Convenience: register a decoder-only implementation built from a
    /// caps + factory pair.
    pub fn register_decoder_impl(
        &mut self,
        id: CodecId,
        caps: CodecCapabilities,
        factory: DecoderFactory,
    ) {
        self.register(
            id,
            CodecImplementation {
                caps: caps.with_decode(),
                make_decoder: Some(factory),
                make_encoder: None,
            },
        );
    }

    /// Convenience: register an encoder-only implementation.
    pub fn register_encoder_impl(
        &mut self,
        id: CodecId,
        caps: CodecCapabilities,
        factory: EncoderFactory,
    ) {
        self.register(
            id,
            CodecImplementation {
                caps: caps.with_encode(),
                make_decoder: None,
                make_encoder: Some(factory),
            },
        );
    }

    /// Convenience: register a single implementation that handles both
    /// decode and encode for a codec id.
    pub fn register_both(
        &mut self,
        id: CodecId,
        caps: CodecCapabilities,
        decode: DecoderFactory,
        encode: EncoderFactory,
    ) {
        self.register(
            id,
            CodecImplementation {
                caps: caps.with_decode().with_encode(),
                make_decoder: Some(decode),
                make_encoder: Some(encode),
            },
        );
    }

    /// Backwards-compat shim: register a decoder-only impl with default
    /// software capabilities. Prefer `register_decoder_impl` for new code.
    pub fn register_decoder(&mut self, id: CodecId, factory: DecoderFactory) {
        let caps = CodecCapabilities::audio(id.as_str()).with_decode();
        self.register_decoder_impl(id, caps, factory);
    }

    /// Backwards-compat shim: register an encoder-only impl with default
    /// software capabilities.
    pub fn register_encoder(&mut self, id: CodecId, factory: EncoderFactory) {
        let caps = CodecCapabilities::audio(id.as_str()).with_encode();
        self.register_encoder_impl(id, caps, factory);
    }

    pub fn has_decoder(&self, id: &CodecId) -> bool {
        self.impls
            .get(id)
            .map(|v| v.iter().any(|i| i.make_decoder.is_some()))
            .unwrap_or(false)
    }

    pub fn has_encoder(&self, id: &CodecId) -> bool {
        self.impls
            .get(id)
            .map(|v| v.iter().any(|i| i.make_encoder.is_some()))
            .unwrap_or(false)
    }

    /// Build a decoder for `params`. Walks all implementations matching the
    /// codec id in increasing priority order, skipping any excluded by the
    /// caller's preferences. Init-time fallback: if a higher-priority impl's
    /// constructor returns an error, the next candidate is tried.
    pub fn make_decoder_with(
        &self,
        params: &CodecParameters,
        prefs: &CodecPreferences,
    ) -> Result<Box<dyn Decoder>> {
        let candidates = self
            .impls
            .get(&params.codec_id)
            .ok_or_else(|| Error::CodecNotFound(params.codec_id.to_string()))?;
        let mut ranked: Vec<&CodecImplementation> = candidates
            .iter()
            .filter(|i| i.make_decoder.is_some() && !prefs.excludes(&i.caps))
            .filter(|i| caps_fit_params(&i.caps, params, false))
            .collect();
        ranked.sort_by_key(|i| prefs.effective_priority(&i.caps));
        let mut last_err: Option<Error> = None;
        for imp in ranked {
            match (imp.make_decoder.unwrap())(params) {
                Ok(d) => return Ok(d),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap_or_else(|| {
            Error::CodecNotFound(format!(
                "no decoder for {} accepts the requested parameters",
                params.codec_id
            ))
        }))
    }

    /// Build an encoder, with the same priority + fallback semantics.
    pub fn make_encoder_with(
        &self,
        params: &CodecParameters,
        prefs: &CodecPreferences,
    ) -> Result<Box<dyn Encoder>> {
        let candidates = self
            .impls
            .get(&params.codec_id)
            .ok_or_else(|| Error::CodecNotFound(params.codec_id.to_string()))?;
        let mut ranked: Vec<&CodecImplementation> = candidates
            .iter()
            .filter(|i| i.make_encoder.is_some() && !prefs.excludes(&i.caps))
            .filter(|i| caps_fit_params(&i.caps, params, true))
            .collect();
        ranked.sort_by_key(|i| prefs.effective_priority(&i.caps));
        let mut last_err: Option<Error> = None;
        for imp in ranked {
            match (imp.make_encoder.unwrap())(params) {
                Ok(e) => return Ok(e),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap_or_else(|| {
            Error::CodecNotFound(format!(
                "no encoder for {} accepts the requested parameters",
                params.codec_id
            ))
        }))
    }

    /// Default-preference shorthand for `make_decoder_with`.
    pub fn make_decoder(&self, params: &CodecParameters) -> Result<Box<dyn Decoder>> {
        self.make_decoder_with(params, &CodecPreferences::default())
    }

    /// Default-preference shorthand for `make_encoder_with`.
    pub fn make_encoder(&self, params: &CodecParameters) -> Result<Box<dyn Encoder>> {
        self.make_encoder_with(params, &CodecPreferences::default())
    }

    /// Iterate codec ids that have at least one decoder implementation.
    pub fn decoder_ids(&self) -> impl Iterator<Item = &CodecId> {
        self.impls
            .iter()
            .filter(|(_, v)| v.iter().any(|i| i.make_decoder.is_some()))
            .map(|(id, _)| id)
    }

    pub fn encoder_ids(&self) -> impl Iterator<Item = &CodecId> {
        self.impls
            .iter()
            .filter(|(_, v)| v.iter().any(|i| i.make_encoder.is_some()))
            .map(|(id, _)| id)
    }

    /// All registered implementations of a given codec id.
    pub fn implementations(&self, id: &CodecId) -> &[CodecImplementation] {
        self.impls.get(id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Iterator over every (codec_id, impl) pair — useful for `oxideav list`
    /// to show capability flags per implementation.
    pub fn all_implementations(&self) -> impl Iterator<Item = (&CodecId, &CodecImplementation)> {
        self.impls
            .iter()
            .flat_map(|(id, v)| v.iter().map(move |i| (id, i)))
    }

    /// Attach a codec id to a container tag so demuxers can look up
    /// the right decoder without each container maintaining its own
    /// hand-written FourCC / WAVEFORMATEX / Matroska table.
    ///
    /// A single tag may be claimed by multiple codecs — the typical
    /// reason is mislabelling in the wild: e.g. AVI FourCC `DIV3`
    /// legally means MS-MPEG4v3 but in practice is applied to real
    /// MPEG-4 Part 2 streams often enough that both `oxideav-msmpeg4`
    /// and `oxideav-mpeg4video` want to claim it, each with a probe
    /// that accepts the bitstream it actually understands.
    ///
    /// Claims are stored sorted by `priority` descending;
    /// [`Self::resolve_tag`] walks them in order and returns the
    /// first whose probe accepts (or the first with no probe).
    pub fn claim_tag(
        &mut self,
        id: CodecId,
        tag: CodecTag,
        priority: u8,
        probe: Option<CodecProbe>,
    ) {
        let entry = self.tag_claims.entry(tag).or_default();
        entry.push((id, TagClaim { priority, probe }));
        // Stable sort — later registrations with equal priority appear
        // after earlier ones, which matches "probe-backed claims come
        // first, catch-all fallbacks last" when priorities are equal.
        entry.sort_by_key(|(_, claim)| std::cmp::Reverse(claim.priority));
    }

    /// Resolve a container tag to a codec id. Walks the priority-
    /// ordered claim list and returns the first matching one:
    ///
    /// * Claim has a probe + `probe_data` is `Some(d)` → accept iff
    ///   `probe(d)` returns true; otherwise skip and try the next.
    /// * Claim has a probe + `probe_data` is `None` → accept
    ///   (probing without bytes is impossible; fall back to priority).
    /// * Claim has no probe → accept (catch-all).
    ///
    /// Returns `None` if the tag has no claimants.
    ///
    /// # Example
    ///
    /// ```
    /// use oxideav_codec::CodecRegistry;
    /// use oxideav_core::{CodecId, CodecTag};
    ///
    /// let mut reg = CodecRegistry::new();
    /// reg.claim_tag(CodecId::new("flac"), CodecTag::fourcc(b"FLAC"), 10, None);
    ///
    /// let resolved = reg.resolve_tag(&CodecTag::fourcc(b"FLAC"), None);
    /// assert_eq!(resolved.map(|c| c.as_str()), Some("flac"));
    /// ```
    pub fn resolve_tag(&self, tag: &CodecTag, probe_data: Option<&[u8]>) -> Option<&CodecId> {
        let claims = self.tag_claims.get(tag)?;
        for (id, claim) in claims {
            match (claim.probe, probe_data) {
                (None, _) => return Some(id),
                (Some(_), None) => return Some(id),
                (Some(p), Some(d)) => {
                    if p(d) {
                        return Some(id);
                    }
                }
            }
        }
        None
    }

    /// Return the full list of claims for a tag in priority order —
    /// useful for diagnostics (`oxideav tags` / error messages when
    /// no claim accepts the probed bytes).
    pub fn claims_for_tag(&self, tag: &CodecTag) -> &[(CodecId, TagClaim)] {
        self.tag_claims
            .get(tag)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Iterator over every tag claim currently registered — used by
    /// `oxideav tags` debug output and by integration tests that want
    /// to verify the full tag surface.
    pub fn all_tag_claims(&self) -> impl Iterator<Item = (&CodecTag, &CodecId, &TagClaim)> {
        self.tag_claims
            .iter()
            .flat_map(|(tag, claims)| claims.iter().map(move |(id, c)| (tag, id, c)))
    }
}

/// Implement the shared [`CodecResolver`] interface so container
/// demuxers can accept `&dyn CodecResolver` without depending on
/// this crate directly — the trait lives in oxideav-core.
impl CodecResolver for CodecRegistry {
    fn resolve_tag(&self, tag: &CodecTag, probe_data: Option<&[u8]>) -> Option<CodecId> {
        // Delegate to the inherent method and clone the result so the
        // trait returns an owned CodecId (the inherent method returns
        // &CodecId tied to the registry's lifetime).
        CodecRegistry::resolve_tag(self, tag, probe_data).cloned()
    }
}

/// Check whether an implementation's restrictions are compatible with the
/// requested codec parameters. `for_encode` swaps the rare cases where a
/// restriction only applies one way.
fn caps_fit_params(caps: &CodecCapabilities, p: &CodecParameters, for_encode: bool) -> bool {
    let _ = for_encode; // reserved for future use (e.g. encode-only bitrate caps)
    if let (Some(max), Some(w)) = (caps.max_width, p.width) {
        if w > max {
            return false;
        }
    }
    if let (Some(max), Some(h)) = (caps.max_height, p.height) {
        if h > max {
            return false;
        }
    }
    if let (Some(max), Some(br)) = (caps.max_bitrate, p.bit_rate) {
        if br > max {
            return false;
        }
    }
    if let (Some(max), Some(sr)) = (caps.max_sample_rate, p.sample_rate) {
        if sr > max {
            return false;
        }
    }
    if let (Some(max), Some(ch)) = (caps.max_channels, p.channels) {
        if ch > max {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tag_tests {
    use super::*;
    use oxideav_core::CodecCapabilities;

    fn looks_like_msmpeg4(data: &[u8]) -> bool {
        // For tests: MS-MPEG4 if no 0x000001 start code in first 8 bytes.
        !data.windows(3).take(6).any(|w| w == [0x00, 0x00, 0x01])
    }

    fn looks_like_mpeg4_part2(data: &[u8]) -> bool {
        data.windows(3).take(6).any(|w| w == [0x00, 0x00, 0x01])
    }

    #[test]
    fn resolve_single_claim_no_probe() {
        let mut reg = CodecRegistry::new();
        reg.claim_tag(CodecId::new("flac"), CodecTag::fourcc(b"FLAC"), 10, None);
        assert_eq!(
            reg.resolve_tag(&CodecTag::fourcc(b"FLAC"), None)
                .map(|c| c.as_str()),
            Some("flac"),
        );
    }

    #[test]
    fn resolve_missing_tag_returns_none() {
        let reg = CodecRegistry::new();
        assert!(reg.resolve_tag(&CodecTag::fourcc(b"????"), None).is_none());
    }

    #[test]
    fn priority_highest_wins() {
        let mut reg = CodecRegistry::new();
        reg.claim_tag(CodecId::new("low"), CodecTag::fourcc(b"TEST"), 1, None);
        reg.claim_tag(CodecId::new("high"), CodecTag::fourcc(b"TEST"), 10, None);
        reg.claim_tag(CodecId::new("mid"), CodecTag::fourcc(b"TEST"), 5, None);
        assert_eq!(
            reg.resolve_tag(&CodecTag::fourcc(b"TEST"), None)
                .map(|c| c.as_str()),
            Some("high"),
        );
    }

    #[test]
    fn probe_chooses_matching_bitstream() {
        // DIV3: msmpeg4v3 claims with "looks like MS" probe, mpeg4video
        // claims with "looks like Part 2" probe. A packet beginning with
        // 0x000001B0 (MPEG-4 Part 2 VOS) must route to mpeg4video even
        // though msmpeg4v3 has the higher priority.
        let mut reg = CodecRegistry::new();
        reg.claim_tag(
            CodecId::new("msmpeg4v3"),
            CodecTag::fourcc(b"DIV3"),
            10,
            Some(looks_like_msmpeg4),
        );
        reg.claim_tag(
            CodecId::new("mpeg4video"),
            CodecTag::fourcc(b"DIV3"),
            5,
            Some(looks_like_mpeg4_part2),
        );

        let mpeg4_part2 = [0x00u8, 0x00, 0x01, 0xB0, 0x01, 0x00];
        let ms_mpeg4 = [0x85u8, 0x3F, 0xD4, 0x80, 0x00, 0xA2];

        assert_eq!(
            reg.resolve_tag(&CodecTag::fourcc(b"DIV3"), Some(&mpeg4_part2))
                .map(|c| c.as_str()),
            Some("mpeg4video"),
        );
        assert_eq!(
            reg.resolve_tag(&CodecTag::fourcc(b"DIV3"), Some(&ms_mpeg4))
                .map(|c| c.as_str()),
            Some("msmpeg4v3"),
        );
    }

    #[test]
    fn probed_claims_without_probe_data_fall_back_to_priority() {
        let mut reg = CodecRegistry::new();
        reg.claim_tag(
            CodecId::new("msmpeg4v3"),
            CodecTag::fourcc(b"DIV3"),
            10,
            Some(looks_like_msmpeg4),
        );
        reg.claim_tag(
            CodecId::new("mpeg4video"),
            CodecTag::fourcc(b"DIV3"),
            5,
            Some(looks_like_mpeg4_part2),
        );
        // No probe_data → highest-priority wins.
        assert_eq!(
            reg.resolve_tag(&CodecTag::fourcc(b"DIV3"), None)
                .map(|c| c.as_str()),
            Some("msmpeg4v3"),
        );
    }

    #[test]
    fn fallback_no_probe_catches_everything() {
        let mut reg = CodecRegistry::new();
        reg.claim_tag(
            CodecId::new("picky"),
            CodecTag::fourcc(b"MAYB"),
            10,
            Some(|_| false), // never accepts
        );
        reg.claim_tag(CodecId::new("fallback"), CodecTag::fourcc(b"MAYB"), 1, None);
        assert_eq!(
            reg.resolve_tag(&CodecTag::fourcc(b"MAYB"), Some(b"hello"))
                .map(|c| c.as_str()),
            Some("fallback"),
        );
    }

    #[test]
    fn claims_for_tag_returns_ordered_list() {
        let mut reg = CodecRegistry::new();
        reg.claim_tag(CodecId::new("a"), CodecTag::fourcc(b"XYZ1"), 1, None);
        reg.claim_tag(CodecId::new("b"), CodecTag::fourcc(b"XYZ1"), 9, None);
        reg.claim_tag(CodecId::new("c"), CodecTag::fourcc(b"XYZ1"), 5, None);
        let claims: Vec<_> = reg
            .claims_for_tag(&CodecTag::fourcc(b"XYZ1"))
            .iter()
            .map(|(id, c)| (id.as_str().to_string(), c.priority))
            .collect();
        assert_eq!(
            claims,
            vec![
                ("b".to_string(), 9),
                ("c".to_string(), 5),
                ("a".to_string(), 1),
            ],
        );
    }

    #[test]
    fn fourcc_case_insensitive_lookup() {
        let mut reg = CodecRegistry::new();
        reg.claim_tag(CodecId::new("vid"), CodecTag::fourcc(b"div3"), 10, None);
        // Registered as "DIV3" (uppercase via ctor); lookup using lowercase
        // also hits thanks to fourcc()-normalisation on lookup side.
        assert!(reg.resolve_tag(&CodecTag::fourcc(b"DIV3"), None).is_some());
        assert!(reg.resolve_tag(&CodecTag::fourcc(b"div3"), None).is_some());
        assert!(reg.resolve_tag(&CodecTag::fourcc(b"DiV3"), None).is_some());
    }

    #[test]
    fn wave_format_and_matroska_tags_work() {
        let mut reg = CodecRegistry::new();
        reg.claim_tag(CodecId::new("mp3"), CodecTag::wave_format(0x0055), 10, None);
        reg.claim_tag(
            CodecId::new("h264"),
            CodecTag::matroska("V_MPEG4/ISO/AVC"),
            10,
            None,
        );
        assert_eq!(
            reg.resolve_tag(&CodecTag::wave_format(0x0055), None)
                .map(|c| c.as_str()),
            Some("mp3"),
        );
        assert_eq!(
            reg.resolve_tag(&CodecTag::matroska("V_MPEG4/ISO/AVC"), None)
                .map(|c| c.as_str()),
            Some("h264"),
        );
    }

    #[test]
    fn mp4_object_type_tag_works() {
        let mut reg = CodecRegistry::new();
        // 0x40 = MPEG-4 AAC per ISO/IEC 14496-1.
        reg.claim_tag(
            CodecId::new("aac"),
            CodecTag::mp4_object_type(0x40),
            10,
            None,
        );
        assert_eq!(
            reg.resolve_tag(&CodecTag::mp4_object_type(0x40), None)
                .map(|c| c.as_str()),
            Some("aac"),
        );
    }

    #[test]
    fn suppress_unused_caps() {
        let _ = CodecCapabilities::audio("dummy");
    }
}
