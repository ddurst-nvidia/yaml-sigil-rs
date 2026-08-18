# yaml-sigil-signing

`yaml-sigil-signing` creates signed YAML and protobuf documents for
[`yaml-sigil`](https://github.com/NVIDIA/yaml-sigil-spec#tldr).

Use this crate to sign payload bytes with Ed25519 or ECDSA P-256 SHA-256 and
emit a `yaml-sigil` artifact. Choose YAML or protobuf output explicitly for
each signing request.

## API Surface

- `sign` is the unified in-process signing entry point.
- `sign_yaml` and `sign_proto` provide form-specific convenience wrappers.
- `sign_with_rng`, `sign_yaml_with_rng`, and `sign_proto_with_rng` accept a
  caller-supplied `CryptoRngCore`.
- `DefaultSigner` and `DefaultAsyncSigner` delegate to the free functions.
- `Signer`, `SignerWithRng`, `AsyncSigner`, `AsyncSignerWithRng`, outcome
  types, capability types, and `CryptoRngCore` are re-exported from
  [`yaml-sigil-traits`](https://crates.io/crates/yaml-sigil-traits).
- `SigningKey` accepts signing keys from
  [`ed25519-dalek`](https://crates.io/crates/ed25519-dalek) and
  [`p256`](https://crates.io/crates/p256). `SignRequest` uses those same key
  types with the request shape defined by `yaml-sigil-traits`.

The shared traits allow implementations to choose different key types. This
crate's free functions and default signers use the RustCrypto types above.

The default `std` feature enables tracing and operating-system entropy.
Ordinary ECDSA signing uses that entropy; ordinary alloc-only signing
advertises Ed25519 only. The caller-RNG APIs advertise both algorithms with or
without `std`. ECDSA consumes 32 bytes through `try_fill_bytes`, uses them to
seed an internal ChaCha20 RNG, and returns `KeyOperationFailure` if entropy
acquisition fails. Ed25519 does not consume the caller RNG.

Disable default features for `no_std + alloc`. The application supplies its
allocator, panic behavior, async executor, and caller RNG. The
`json-schema-validate` feature implies `std`. Rust 1.95.0 compile support is
checked for `thumbv7em-none-eabi`.

`SigningKey` debug output is redacted by design. Do not log private keys, seed
material, tokens, or raw signatures on trusted fact surfaces.
