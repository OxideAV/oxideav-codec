# oxideav-codec

Codec traits + registry for the
[oxideav](https://github.com/OxideAV/oxideav-workspace) pure-Rust media
framework. Every per-format codec crate (MP3, AAC, H.264, GIF, …)
implements `Decoder` and/or `Encoder` and registers itself via a
`CodecImplementation`; the aggregator builds one `CodecRegistry` with
the union of every enabled feature.

* **`Decoder`** — `send_packet` → `receive_frame`, plus `flush` (EOS
  drain) and `reset` (state wipe after seek, added in 0.0.4).
* **`Encoder`** — `send_frame` → `receive_packet`, plus `flush`.
* **`CodecCapabilities`** — per-impl flags (lossless, intra-only,
  accepted pixel formats, supported sample rates, priority) so
  registries can pick the right implementation for a given request.
* **`CodecRegistry`** — factory lookup by `CodecId`, with fallback when
  the first-choice implementation refuses the input.

Zero C dependencies. Zero FFI.

## Usage

```toml
[dependencies]
oxideav-codec = "0.0"
```

Typical pattern in a codec crate:

```rust
pub fn register(reg: &mut oxideav_codec::CodecRegistry) {
    let caps = CodecCapabilities::audio("my_codec_sw")
        .with_lossless(true);
    reg.register_both(
        CodecId::new("my-codec"),
        caps,
        make_decoder,
        make_encoder,
    );
}
```

## License

MIT — see [LICENSE](LICENSE).
