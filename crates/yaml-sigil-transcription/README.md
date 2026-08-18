# yaml-sigil-transcription

`yaml-sigil-transcription` combines document and signature components into
[`yaml-sigil`](https://github.com/NVIDIA/yaml-sigil-spec#tldr) documents and
separates existing documents back into those components. It supports YAML and
protobuf forms.

In the API, the document bytes are the `payload` and the encoded signature
component is the `signature_carrier`. Compose joins them into an artifact,
while decompose returns their byte ranges. These operations change document
structure only. They do not verify a signature or authenticate the payload. Use
[`yaml-sigil-verification`](https://crates.io/crates/yaml-sigil-verification)
for signature verification.

YAML Compose requires payload bytes that form a valid UTF-8 stream without a
BOM and with a final line terminator when non-empty. Protobuf Compose treats
payload bytes as opaque and preserves every accepted byte unchanged.

## API Surface

- `compose` and `decompose` perform the byte operations.
- `DefaultTranscriber` and `DefaultAsyncTranscriber` delegate to the free
  functions.
- `Transcriber`, `AsyncTranscriber`, request types, response types, and
  capability types are re-exported from
  [`yaml-sigil-traits`](https://crates.io/crates/yaml-sigil-traits).

This crate does not provide RPC transport. Consumers that need a service
boundary should wire the trait API into their own deployment.

## Feature Configuration

The default `std` feature enables tracing and propagates standard-library
support through dependencies. Disable default features for `no_std + alloc`.
The application supplies the allocator, panic behavior, and any executor used
to poll async trait methods. Rust 1.95.0 compile support is checked for
`thumbv7em-none-eabi`.
