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

## Using a codec directly (standalone, no container, no pipeline)

OxideAV's codecs are designed to be usable on their own. If you already
have raw encoded packets (from disk, the network, or another parser),
you only need three crates: `oxideav-core` for the `Packet` / `Frame`
types, `oxideav-codec` for the `Decoder` / `Encoder` traits + registry,
and the specific codec crate you want.

Example — decoding G.711 µ-law:

```toml
[dependencies]
oxideav-core = "0.0"
oxideav-codec = "0.0"
oxideav-g711 = "0.0"
```

```rust
use oxideav_codec::CodecRegistry;
use oxideav_core::{CodecId, CodecParameters, Frame, Packet, TimeBase};

// 1. Register the codec with a registry (one-time setup).
let mut reg = CodecRegistry::new();
oxideav_g711::register(&mut reg);

// 2. Describe the stream — here 8 kHz mono µ-law.
let mut params = CodecParameters::audio(CodecId::new("pcm_mulaw"));
params.sample_rate = Some(8_000);
params.channels = Some(1);

// 3. Build a decoder.
let mut dec = reg.make_decoder(&params)?;

// 4. Feed packets + pull frames in a loop.
let pkt = Packet::new(/* stream_index */ 0, TimeBase::new(1, 8_000), mulaw_bytes);
dec.send_packet(&pkt)?;

match dec.receive_frame()? {
    Frame::Audio(a) => {
        // `a.data[0]` is interleaved S16 PCM; `a.channels` / `a.samples`
        // / `a.sample_rate` describe the shape.
    }
    _ => unreachable!("G.711 yields audio"),
}
# Ok::<(), oxideav_core::Error>(())
```

### The packet → frame loop

`send_packet` / `receive_frame` is a two-step state machine. Most
codecs accept exactly one packet per frame (a 1:1 mapping), so the
pattern is:

```rust
for pkt in stream_of_packets {
    dec.send_packet(&pkt)?;
    let frame = dec.receive_frame()?;
    // ... use frame ...
}
```

Some codecs buffer internally (e.g. a video codec waiting for reference
frames). In that case `receive_frame` returns `Error::NeedMore` until
the decoder has enough input; keep sending packets until you get a
frame, and keep pulling frames after each send (a single packet may
produce multiple frames for some codecs). The canonical loop:

```rust
loop {
    match dec.receive_frame() {
        Ok(frame) => { /* consume */ }
        Err(oxideav_core::Error::NeedMore) => break, // send more packets
        Err(oxideav_core::Error::Eof) => return Ok(()),
        Err(e) => return Err(e),
    }
}
```

### Signalling end-of-stream

When you've sent every packet and want to flush any still-buffered
output:

```rust
dec.flush()?;
while let Ok(frame) = dec.receive_frame() { /* trailing frames */ }
```

### Resetting after a seek

If you reposition the input mid-stream (seek), call `reset()` instead
of `flush()`. `reset` drops buffered packets *and* wipes codec-internal
state (LPC memory, IMDCT overlap, reference pictures) so the first
frame after the seek decodes cleanly rather than glitching:

```rust
// After repositioning your byte stream:
dec.reset()?;
// ...resume feeding packets from the new position...
```

### Encoding is symmetric

The `Encoder` trait mirrors `Decoder` — build via
`reg.make_encoder(&params)`, feed raw `Frame`s with `send_frame`, pull
encoded `Packet`s with `receive_packet`. See each codec's README for
per-format parameter requirements (sample rate, channel count,
bitrate, pixel format, etc.).

## Extending the registry

Codec crates expose a single `register(reg)` function that pushes their
implementation(s) into the registry. The registry supports multiple
implementations per codec id with priority + capability fallback, so
you can register a fast-but-lossy impl and a slow-but-accurate one and
have the registry pick based on `CodecPreferences`:

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
