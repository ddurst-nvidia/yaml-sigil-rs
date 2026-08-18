// SPDX-FileCopyrightText: Copyright 2026 NVIDIA CORPORATION & AFFILIATES
// SPDX-License-Identifier: Apache-2.0

//! YamlSigil v1alpha1 signing: YAML + protobuf artifacts, Ed25519 + ECDSA P-256 SHA-256.
//! Signed-artifact transcoding lives in [`transcription`].
//!
//! Request-shape failures are [`SignInvocationError`]; sign-time failures are
//! [`SignError`], plus output-path extensions (`YamlSerialize` only; protobuf
//! encode is infallible here).
//!
//! Convenience wrappers [`sign_yaml`] and [`sign_proto`] call [`sign`] with a fixed [`OutputForm`].
//! Their caller-RNG counterparts are [`sign_yaml_with_rng`] and
//! [`sign_proto_with_rng`].

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;
#[cfg(all(test, not(feature = "std")))]
extern crate std;

mod proto_carrier;
pub mod transcription;

pub use transcription::{
    TranscodeError, proto_wire_to_signed_yaml_stream, signed_yaml_stream_to_proto_wire,
};

use alloc::{string::ToString, vec::Vec};
use yaml_sigil_core::{SignatureDocument, validate_payload_stream};
use yaml_sigil_traits::{
    AlgorithmId, ProtobufWireDecodeAdvertisement, YamlSignatureDocumentDuplicateKeyPolicy,
    YamlSignatureDocumentUnknownFieldPolicy,
};

pub use yaml_sigil_traits::CryptoRngCore;

// The portable traits and DTOs live in `yaml-sigil-traits`. This implementation
// binds the generic key-bearing DTOs to its RustCrypto key types while retaining
// the established `yaml_sigil_signing::{SigningKey, SignRequest}` paths.
pub use yaml_sigil_traits::signing::{
    AsyncSigner, AsyncSignerWithRng, OutputForm, SignError, SignInvocationError, SignOutcome,
    SignSuccess, Signer, SignerCapabilities, SignerWithRng,
};
use yaml_sigil_traits::signing::{
    SignRequest as GenericSignRequest, SigningKey as GenericSigningKey,
};

/// Signing keys supported by this RustCrypto implementation.
pub type SigningKey<'a> = GenericSigningKey<'a, ed25519_dalek::SigningKey, p256::ecdsa::SigningKey>;

/// Unified sign request specialized for this RustCrypto implementation.
pub type SignRequest<'a> =
    GenericSignRequest<'a, ed25519_dalek::SigningKey, p256::ecdsa::SigningKey>;

/// Return the capability set for ordinary signing in this crate build.
///
/// Standard-library builds advertise both algorithms. Alloc-only builds
/// advertise Ed25519; use [`signer_capabilities_with_rng`] for the algorithms
/// available with caller-supplied randomness.
pub fn signer_capabilities() -> SignerCapabilities {
    #[cfg(feature = "std")]
    const SUPPORTED_ALGORITHMS: &[AlgorithmId] =
        &[AlgorithmId::Ed25519, AlgorithmId::EcdsaP256Sha256];
    #[cfg(not(feature = "std"))]
    const SUPPORTED_ALGORITHMS: &[AlgorithmId] = &[AlgorithmId::Ed25519];

    signer_capabilities_for(SUPPORTED_ALGORITHMS)
}

/// Return the capability set available with caller-supplied randomness.
pub fn signer_capabilities_with_rng() -> SignerCapabilities {
    signer_capabilities_for(&[AlgorithmId::Ed25519, AlgorithmId::EcdsaP256Sha256])
}

fn signer_capabilities_for(supported_algorithms: &'static [AlgorithmId]) -> SignerCapabilities {
    SignerCapabilities {
        protobuf_wire_decode: ProtobufWireDecodeAdvertisement::UnprofiledStockDecoder,
        yaml_signature_duplicate_key_policy:
            YamlSignatureDocumentDuplicateKeyPolicy::RejectedAtParse,
        yaml_signature_unknown_field_policy:
            YamlSignatureDocumentUnknownFieldPolicy::RejectedAtParse,
        supported_output_forms: &[OutputForm::Yaml, OutputForm::Protobuf],
        supported_algorithms,
        best_effort_yaml_validation: false,
        implementation_name: env!("CARGO_PKG_NAME"),
        implementation_version: env!("CARGO_PKG_VERSION"),
    }
}

/// Parameters for producing a signed YAML artifact (convenience wrapper).
pub struct SignYamlParams<'a> {
    pub payload: &'a [u8],
    pub algorithm: AlgorithmId,
    pub key: SigningKey<'a>,
    pub keyid: Option<&'a str>,
    /// If true, a missing trailing `\\n` on a non-empty payload is fixed by appending `0x0A`.
    pub append_missing_final_newline: bool,
}

/// Parameters for producing protobuf `SignedYamlArtifact` wire bytes (convenience wrapper).
pub struct SignProtoParams<'a> {
    pub payload: &'a [u8],
    pub algorithm: AlgorithmId,
    pub key: SigningKey<'a>,
    pub keyid: Option<&'a str>,
    /// Ignored for protobuf output; payload bytes are always preserved exactly.
    pub append_missing_final_newline: bool,
}

fn validate_invocation(
    req: &SignRequest<'_>,
    caps: &SignerCapabilities,
) -> Result<(), SignInvocationError> {
    if !caps.supported_output_forms.contains(&req.output_form) {
        return Err(SignInvocationError::InvalidOrUnsupportedOutputForm);
    }
    if !caps.supported_algorithms.contains(&req.algorithm) {
        return Err(SignInvocationError::InvalidOrUnsupportedAlgorithm);
    }
    if !req.algorithm_parameters.is_empty() {
        return Err(SignInvocationError::InvalidAlgorithmParameters);
    }
    if let Some(keyid) = req.keyid {
        let octets = keyid.len();
        if octets == 0 || octets > 1024 || keyid.contains(['\r', '\n']) {
            return Err(SignInvocationError::InvalidKeyid);
        }
    }
    match (&req.algorithm, &req.key) {
        (AlgorithmId::Ed25519, SigningKey::Ed25519(_)) => Ok(()),
        (AlgorithmId::EcdsaP256Sha256, SigningKey::EcdsaP256Sha256(_)) => Ok(()),
        _ => Err(SignInvocationError::InvalidOrUnsupportedAlgorithm),
    }
}

fn normalize_yaml_payload(
    payload: &[u8],
    append_missing_final_newline: bool,
) -> Result<Vec<u8>, SignError> {
    if core::str::from_utf8(payload).is_err() {
        return Err(SignError::InvalidPayloadBytes);
    }
    if payload.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Err(SignError::InvalidPayloadBytes);
    }
    if payload.is_empty() {
        return Ok(Vec::new());
    }
    if payload.ends_with(b"\n") {
        return Ok(payload.to_vec());
    }
    if append_missing_final_newline {
        let mut v = payload.to_vec();
        v.push(b'\n');
        return Ok(v);
    }
    Err(SignError::PayloadLineTerminatorRefusal)
}

/// Unified signing entry point corresponding to the IDL `Sign` operation.
///
/// For protobuf output, `append_missing_final_newline` is
/// ignored and the payload bytes are signed and emitted without modification.
///
/// Standard-library builds obtain operating-system entropy for ECDSA.
/// Alloc-only builds accept Ed25519 through this entry point; use
/// [`sign_with_rng`] for ECDSA.
#[cfg_attr(
    feature = "std",
    tracing::instrument(level = "info", skip(req), fields(alg = ?req.algorithm, form = ?req.output_form))
)]
pub fn sign(req: &SignRequest<'_>) -> SignOutcome {
    #[cfg(feature = "std")]
    {
        let mut rng = rand_core::OsRng;
        sign_inner(req, &signer_capabilities(), Some(&mut rng))
    }
    #[cfg(not(feature = "std"))]
    {
        sign_inner(req, &signer_capabilities(), None)
    }
}

/// Unified signing entry point with caller-supplied cryptographic randomness.
///
/// ECDSA obtains 32 bytes with [`CryptoRngCore::try_fill_bytes`] and maps an
/// entropy failure to [`SignError::KeyOperationFailure`]. Ed25519 does not
/// consume `rng`.
#[cfg_attr(
    feature = "std",
    tracing::instrument(level = "info", skip(req, rng), fields(alg = ?req.algorithm, form = ?req.output_form))
)]
pub fn sign_with_rng(req: &SignRequest<'_>, rng: &mut dyn CryptoRngCore) -> SignOutcome {
    sign_inner(req, &signer_capabilities_with_rng(), Some(rng))
}

fn sign_inner(
    req: &SignRequest<'_>,
    capabilities: &SignerCapabilities,
    rng: Option<&mut dyn CryptoRngCore>,
) -> SignOutcome {
    if let Err(e) = validate_invocation(req, capabilities) {
        return SignOutcome::Invocation(e);
    }

    // Only YAML output applies the YAML envelope rules: valid UTF-8, no BOM,
    // and a final line terminator. Protobuf payloads are opaque bytes and must
    // bypass both normalization and validation.
    let payload = match req.output_form {
        OutputForm::Yaml => {
            let payload =
                match normalize_yaml_payload(req.payload, req.append_missing_final_newline) {
                    Ok(p) => p,
                    Err(e) => return SignOutcome::Signer(e),
                };
            if validate_payload_stream(&payload).is_err() {
                return SignOutcome::Signer(SignError::InvalidPayloadBytes);
            }
            payload
        }
        OutputForm::Protobuf => req.payload.to_vec(),
    };

    let modified_payload = if req.payload == payload.as_slice() {
        Vec::new()
    } else {
        payload.clone()
    };

    let sig_bytes = match sign_digest(&payload, req.algorithm, &req.key, rng) {
        Ok(b) => b,
        Err(e) => return SignOutcome::Signer(e),
    };

    let artifact = match req.output_form {
        OutputForm::Yaml => match emit_yaml_artifact(&payload, req, &sig_bytes) {
            Ok(a) => a,
            Err(e) => return SignOutcome::Signer(e),
        },
        OutputForm::Protobuf => match emit_proto_artifact(&payload, req, &sig_bytes) {
            Ok(a) => a,
            Err(e) => return SignOutcome::Signer(e),
        },
    };

    SignOutcome::Success(SignSuccess {
        artifact,
        modified_payload,
    })
}

fn emit_yaml_artifact(
    payload: &[u8],
    req: &SignRequest<'_>,
    sig_bytes: &[u8],
) -> Result<Vec<u8>, SignError> {
    let doc = SignatureDocument {
        schema: yaml_sigil_core::SCHEMA_V1ALPHA1.to_string(),
        alg: req.algorithm.as_yaml_str().to_string(),
        keyid: req.keyid.map(|s| s.to_string()),
        signature: base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            sig_bytes,
        ),
    };

    let mut body = yaml_sigil_core::serialize_signature_document(&doc)
        .map_err(|e| SignError::YamlSerialize(e.to_string()))?;
    if !body.ends_with('\n') {
        body.push('\n');
    }

    match yaml_sigil_transcription::compose(&yaml_sigil_transcription::ComposeRequest {
        payload,
        signature_carrier: body.as_bytes(),
        form: yaml_sigil_transcription::TranscriptionForm::Yaml,
    }) {
        yaml_sigil_transcription::ComposeOutcome::Success(s) => Ok(s.artifact),
        yaml_sigil_transcription::ComposeOutcome::Invocation(_)
        | yaml_sigil_transcription::ComposeOutcome::Error(_) => {
            Err(SignError::YamlSerialize("compose failed".into()))
        }
    }
}

fn emit_proto_artifact(
    payload: &[u8],
    req: &SignRequest<'_>,
    sig_bytes: &[u8],
) -> Result<Vec<u8>, SignError> {
    let carrier = proto_carrier::encode_inner_signature_carrier(
        req.algorithm,
        sig_bytes.to_vec(),
        req.keyid.map(|s| s.to_string()),
    );
    Ok(yaml_sigil_core::compose_proto_outer(payload, &carrier))
}

/// Convenience: sign with YAML output (thin wrapper over [`sign`]).
#[cfg_attr(
    feature = "std",
    tracing::instrument(level = "info", skip(params), fields(alg = ?params.algorithm))
)]
pub fn sign_yaml(params: &SignYamlParams<'_>) -> Result<Vec<u8>, SignError> {
    let req = SignRequest {
        payload: params.payload,
        algorithm: params.algorithm,
        key: params.key,
        keyid: params.keyid,
        append_missing_final_newline: params.append_missing_final_newline,
        output_form: OutputForm::Yaml,
        algorithm_parameters: &[],
    };
    match sign(&req) {
        SignOutcome::Success(s) => Ok(s.artifact),
        SignOutcome::Invocation(e) => Err(map_invocation_to_sign_error(e)),
        SignOutcome::Signer(e) => Err(e),
    }
}

/// Convenience: sign with YAML output and caller-supplied randomness.
#[cfg_attr(
    feature = "std",
    tracing::instrument(level = "info", skip(params, rng), fields(alg = ?params.algorithm))
)]
pub fn sign_yaml_with_rng(
    params: &SignYamlParams<'_>,
    rng: &mut dyn CryptoRngCore,
) -> Result<Vec<u8>, SignError> {
    let req = SignRequest {
        payload: params.payload,
        algorithm: params.algorithm,
        key: params.key,
        keyid: params.keyid,
        append_missing_final_newline: params.append_missing_final_newline,
        output_form: OutputForm::Yaml,
        algorithm_parameters: &[],
    };
    match sign_with_rng(&req, rng) {
        SignOutcome::Success(s) => Ok(s.artifact),
        SignOutcome::Invocation(e) => Err(map_invocation_to_sign_error(e)),
        SignOutcome::Signer(e) => Err(e),
    }
}

/// Convenience: sign with protobuf output (thin wrapper over [`sign`]).
#[cfg_attr(
    feature = "std",
    tracing::instrument(level = "info", skip(params), fields(alg = ?params.algorithm))
)]
pub fn sign_proto(params: &SignProtoParams<'_>) -> Result<Vec<u8>, SignError> {
    let req = SignRequest {
        payload: params.payload,
        algorithm: params.algorithm,
        key: params.key,
        keyid: params.keyid,
        append_missing_final_newline: params.append_missing_final_newline,
        output_form: OutputForm::Protobuf,
        algorithm_parameters: &[],
    };
    match sign(&req) {
        SignOutcome::Success(s) => Ok(s.artifact),
        SignOutcome::Invocation(e) => Err(map_invocation_to_sign_error(e)),
        SignOutcome::Signer(e) => Err(e),
    }
}

/// Convenience: sign with protobuf output and caller-supplied randomness.
#[cfg_attr(
    feature = "std",
    tracing::instrument(level = "info", skip(params, rng), fields(alg = ?params.algorithm))
)]
pub fn sign_proto_with_rng(
    params: &SignProtoParams<'_>,
    rng: &mut dyn CryptoRngCore,
) -> Result<Vec<u8>, SignError> {
    let req = SignRequest {
        payload: params.payload,
        algorithm: params.algorithm,
        key: params.key,
        keyid: params.keyid,
        append_missing_final_newline: params.append_missing_final_newline,
        output_form: OutputForm::Protobuf,
        algorithm_parameters: &[],
    };
    match sign_with_rng(&req, rng) {
        SignOutcome::Success(s) => Ok(s.artifact),
        SignOutcome::Invocation(e) => Err(map_invocation_to_sign_error(e)),
        SignOutcome::Signer(e) => Err(e),
    }
}

fn map_invocation_to_sign_error(e: SignInvocationError) -> SignError {
    match e {
        SignInvocationError::InvalidOrUnsupportedAlgorithm => {
            SignError::InvalidOrUnsupportedAlgorithm
        }
        SignInvocationError::InvalidAlgorithmParameters => SignError::InvalidAlgorithmParameters,
        SignInvocationError::InvalidOrUnsupportedOutputForm => {
            SignError::InvalidOrUnsupportedOutputForm
        }
        SignInvocationError::InvalidKeyid => SignError::InvalidKeyid,
    }
}

fn sign_digest(
    payload: &[u8],
    algorithm: AlgorithmId,
    key: &SigningKey<'_>,
    rng: Option<&mut dyn CryptoRngCore>,
) -> Result<Vec<u8>, SignError> {
    match (algorithm, key) {
        (AlgorithmId::Ed25519, SigningKey::Ed25519(sk)) => {
            use ed25519_dalek::Signer;
            Ok(sk.sign(payload).to_bytes().to_vec())
        }
        (AlgorithmId::EcdsaP256Sha256, SigningKey::EcdsaP256Sha256(sk)) => {
            use p256::ecdsa::signature::RandomizedSigner;
            use rand_core::SeedableRng;

            let rng = rng.ok_or(SignError::KeyOperationFailure)?;
            let mut seed = [0u8; 32];
            rng.try_fill_bytes(&mut seed)
                .map_err(|_| SignError::KeyOperationFailure)?;
            let mut nonce_rng = rand_chacha::ChaCha20Rng::from_seed(seed);
            let sig: p256::ecdsa::Signature = sk
                .try_sign_with_rng(&mut nonce_rng, payload)
                .map_err(|_| SignError::KeyOperationFailure)?;
            // Raw R || S 64 octets.
            Ok(sig.to_bytes().to_vec())
        }
        _ => Err(SignError::InvalidOrUnsupportedAlgorithm),
    }
}

/// In-process default signer that delegates to the crate's free functions.
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultSigner;

impl Signer for DefaultSigner {
    type Ed25519SigningKey = ed25519_dalek::SigningKey;
    type P256SigningKey = p256::ecdsa::SigningKey;

    fn capabilities(&self) -> SignerCapabilities {
        signer_capabilities()
    }
    fn sign(&self, req: &SignRequest<'_>) -> SignOutcome {
        sign(req)
    }
}

impl SignerWithRng for DefaultSigner {
    fn capabilities_with_rng(&self) -> SignerCapabilities {
        signer_capabilities_with_rng()
    }

    fn sign_with_rng(&self, req: &SignRequest<'_>, rng: &mut dyn CryptoRngCore) -> SignOutcome {
        sign_with_rng(req, rng)
    }
}

/// In-process default async signer that delegates to the crate's free functions.
///
/// The body delegates directly — no `tokio::spawn_blocking`. The signing path
/// is CPU-bound and short; offloading to a blocking pool
/// would add latency without protecting any meaningful reactor.
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultAsyncSigner;

impl AsyncSigner for DefaultAsyncSigner {
    type Ed25519SigningKey = ed25519_dalek::SigningKey;
    type P256SigningKey = p256::ecdsa::SigningKey;

    fn capabilities(&self) -> SignerCapabilities {
        signer_capabilities()
    }
    async fn sign(&self, req: &SignRequest<'_>) -> SignOutcome {
        sign(req)
    }
}

impl AsyncSignerWithRng for DefaultAsyncSigner {
    fn capabilities_with_rng(&self) -> SignerCapabilities {
        signer_capabilities_with_rng()
    }

    async fn sign_with_rng(
        &self,
        req: &SignRequest<'_>,
        rng: &mut (dyn CryptoRngCore + Send),
    ) -> SignOutcome {
        sign_with_rng(req, rng)
    }
}

#[cfg(test)]
mod tests {
    use core::num::NonZeroU32;

    use super::*;
    use ed25519_dalek::SigningKey as EdSk;
    use p256::ecdsa::{SigningKey as P256Sk, VerifyingKey as P256Vk};
    use rand_core::{CryptoRng, Error as RngError, RngCore};
    use yaml_sigil_verification::{
        ArtifactForm, PublicKeys, VerifierOptions, VerifierState, verify,
    };

    struct PatternRng {
        byte: u8,
        calls: usize,
    }

    impl PatternRng {
        fn new(byte: u8) -> Self {
            Self { byte, calls: 0 }
        }
    }

    impl RngCore for PatternRng {
        fn next_u32(&mut self) -> u32 {
            let mut bytes = [0u8; 4];
            self.fill_bytes(&mut bytes);
            u32::from_le_bytes(bytes)
        }

        fn next_u64(&mut self) -> u64 {
            let mut bytes = [0u8; 8];
            self.fill_bytes(&mut bytes);
            u64::from_le_bytes(bytes)
        }

        fn fill_bytes(&mut self, dest: &mut [u8]) {
            self.calls += 1;
            dest.fill(self.byte);
        }

        fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), RngError> {
            self.fill_bytes(dest);
            Ok(())
        }
    }

    impl CryptoRng for PatternRng {}

    struct FailingRng;

    impl RngCore for FailingRng {
        fn next_u32(&mut self) -> u32 {
            panic!("signing must use fallible RNG access")
        }

        fn next_u64(&mut self) -> u64 {
            panic!("signing must use fallible RNG access")
        }

        fn fill_bytes(&mut self, _dest: &mut [u8]) {
            panic!("signing must use fallible RNG access")
        }

        fn try_fill_bytes(&mut self, _dest: &mut [u8]) -> Result<(), RngError> {
            Err(RngError::from(NonZeroU32::new(1).expect("nonzero")))
        }
    }

    impl CryptoRng for FailingRng {}

    struct PanicRng;

    impl RngCore for PanicRng {
        fn next_u32(&mut self) -> u32 {
            panic!("Ed25519 must not consume caller randomness")
        }

        fn next_u64(&mut self) -> u64 {
            panic!("Ed25519 must not consume caller randomness")
        }

        fn fill_bytes(&mut self, _dest: &mut [u8]) {
            panic!("Ed25519 must not consume caller randomness")
        }

        fn try_fill_bytes(&mut self, _dest: &mut [u8]) -> Result<(), RngError> {
            panic!("Ed25519 must not consume caller randomness")
        }
    }

    impl CryptoRng for PanicRng {}

    fn p256_request<'a>(key: &'a P256Sk) -> SignRequest<'a> {
        SignRequest {
            payload: b"a: b\n",
            algorithm: AlgorithmId::EcdsaP256Sha256,
            key: SigningKey::EcdsaP256Sha256(key),
            keyid: Some("p256-test"),
            append_missing_final_newline: false,
            output_form: OutputForm::Protobuf,
            algorithm_parameters: &[],
        }
    }

    fn success_artifact(outcome: SignOutcome) -> Vec<u8> {
        match outcome {
            SignOutcome::Success(success) => success.artifact,
            other => panic!("expected signing success, got {other:?}"),
        }
    }

    #[test]
    fn signer_capabilities_match_entropy_availability() {
        let c = signer_capabilities();
        #[cfg(feature = "std")]
        assert_eq!(
            c.supported_algorithms,
            &[AlgorithmId::Ed25519, AlgorithmId::EcdsaP256Sha256]
        );
        #[cfg(not(feature = "std"))]
        assert_eq!(c.supported_algorithms, &[AlgorithmId::Ed25519]);
        assert_eq!(c.supported_output_forms.len(), 2);
        assert!(!c.best_effort_yaml_validation);
        assert_eq!(
            c.protobuf_wire_decode,
            yaml_sigil_core::ProtobufWireDecodeAdvertisement::UnprofiledStockDecoder
        );
        assert_eq!(
            c.yaml_signature_duplicate_key_policy,
            yaml_sigil_core::YamlSignatureDocumentDuplicateKeyPolicy::RejectedAtParse
        );
        assert_eq!(
            c.yaml_signature_unknown_field_policy,
            yaml_sigil_core::YamlSignatureDocumentUnknownFieldPolicy::RejectedAtParse
        );

        assert_eq!(
            signer_capabilities_with_rng().supported_algorithms,
            &[AlgorithmId::Ed25519, AlgorithmId::EcdsaP256Sha256]
        );
    }

    #[test]
    fn caller_rng_ecdsa_signatures_verify_and_vary() {
        let sk = P256Sk::from_slice(&[7u8; 32]).expect("valid test key");
        let req = p256_request(&sk);
        let mut first_rng = PatternRng::new(0x11);
        let mut second_rng = PatternRng::new(0x22);

        let first = success_artifact(sign_with_rng(&req, &mut first_rng));
        let second = success_artifact(sign_with_rng(&req, &mut second_rng));
        assert_eq!(first_rng.calls, 1);
        assert_eq!(second_rng.calls, 1);
        assert_ne!(
            first, second,
            "distinct caller entropy must vary ECDSA output"
        );

        let vk = P256Vk::from(&sk);
        let keys = PublicKeys {
            ed25519: None,
            p256: Some(&vk),
        };
        for artifact in [&first, &second] {
            let state = verify(
                artifact,
                ArtifactForm::Proto,
                &keys,
                VerifierOptions::default(),
            )
            .expect("generated artifact verifies without invocation failure");
            assert!(matches!(state, VerifierState::Verified { .. }));
        }
    }

    #[test]
    fn caller_rng_failure_maps_to_key_operation_failure() {
        let sk = P256Sk::from_slice(&[7u8; 32]).expect("valid test key");
        let mut rng = FailingRng;
        assert!(matches!(
            sign_with_rng(&p256_request(&sk), &mut rng),
            SignOutcome::Signer(SignError::KeyOperationFailure)
        ));
    }

    #[test]
    fn ed25519_does_not_consume_caller_rng() {
        let sk = EdSk::from_bytes(&[5u8; 32]);
        let req = SignRequest {
            payload: b"a: b\n",
            algorithm: AlgorithmId::Ed25519,
            key: SigningKey::Ed25519(&sk),
            keyid: None,
            append_missing_final_newline: false,
            output_form: OutputForm::Protobuf,
            algorithm_parameters: &[],
        };
        let mut rng = PanicRng;
        assert!(matches!(
            sign_with_rng(&req, &mut rng),
            SignOutcome::Success(_)
        ));
    }

    #[cfg(not(feature = "std"))]
    #[test]
    fn ordinary_alloc_only_signing_rejects_ecdsa() {
        let sk = P256Sk::from_slice(&[7u8; 32]).expect("valid test key");
        assert!(matches!(
            sign(&p256_request(&sk)),
            SignOutcome::Invocation(SignInvocationError::InvalidOrUnsupportedAlgorithm)
        ));
    }

    #[test]
    fn caller_rng_signer_trait_is_object_safe() {
        let sk = P256Sk::from_slice(&[7u8; 32]).expect("valid test key");
        let signer: &dyn SignerWithRng<
            Ed25519SigningKey = ed25519_dalek::SigningKey,
            P256SigningKey = p256::ecdsa::SigningKey,
        > = &DefaultSigner;
        let mut rng = PatternRng::new(0x33);
        assert_eq!(signer.capabilities_with_rng().supported_algorithms.len(), 2);
        assert!(matches!(
            signer.sign_with_rng(&p256_request(&sk), &mut rng),
            SignOutcome::Success(_)
        ));
    }

    #[cfg(feature = "std")]
    #[test]
    fn ordinary_ecdsa_signing_uses_os_entropy() {
        let sk = P256Sk::from_slice(&[7u8; 32]).expect("valid test key");
        let req = p256_request(&sk);
        let first = success_artifact(sign(&req));
        let second = success_artifact(sign(&req));
        assert_ne!(
            first, second,
            "ordinary ECDSA signatures must be randomized"
        );
    }

    // The concrete RustCrypto bindings must remain expressible on a
    // synchronous trait object.
    #[test]
    fn default_signer_supports_a_trait_object_with_explicit_bindings() {
        let signer: &dyn Signer<
            Ed25519SigningKey = ed25519_dalek::SigningKey,
            P256SigningKey = p256::ecdsa::SigningKey,
        > = &DefaultSigner;
        assert_eq!(signer.capabilities(), signer_capabilities());
    }

    #[test]
    fn signer_rejects_line_break_in_keyid() {
        let sk = EdSk::from_bytes(&[5u8; 32]);
        for keyid in ["kid\nsuffix", "kid\rsuffix"] {
            let req = SignRequest {
                payload: b"a: b\n",
                algorithm: AlgorithmId::Ed25519,
                key: SigningKey::Ed25519(&sk),
                keyid: Some(keyid),
                append_missing_final_newline: false,
                output_form: OutputForm::Yaml,
                algorithm_parameters: &[],
            };
            assert!(matches!(
                sign(&req),
                SignOutcome::Invocation(SignInvocationError::InvalidKeyid)
            ));
        }
    }

    #[test]
    fn default_signer_matches_free_function() {
        let sk = EdSk::from_bytes(&[5u8; 32]);
        let payload = b"a: b\n";
        let req = SignRequest {
            payload,
            algorithm: AlgorithmId::Ed25519,
            key: SigningKey::Ed25519(&sk),
            keyid: Some("kid-d"),
            append_missing_final_newline: false,
            output_form: OutputForm::Yaml,
            algorithm_parameters: &[],
        };
        let direct = match sign(&req) {
            SignOutcome::Success(s) => s.artifact,
            _ => panic!("expected success via free fn"),
        };
        let via_trait = match Signer::sign(&DefaultSigner, &req) {
            SignOutcome::Success(s) => s.artifact,
            _ => panic!("expected success via trait"),
        };
        assert_eq!(direct, via_trait);
        assert_eq!(
            DefaultSigner.capabilities().supported_algorithms.len(),
            signer_capabilities().supported_algorithms.len()
        );
    }

    #[test]
    fn unified_sign_matches_wrappers() {
        let sk = EdSk::from_bytes(&[3u8; 32]);
        let payload = b"x: y\n";
        let yaml_req = SignRequest {
            payload,
            algorithm: AlgorithmId::Ed25519,
            key: SigningKey::Ed25519(&sk),
            keyid: None,
            append_missing_final_newline: false,
            output_form: OutputForm::Yaml,
            algorithm_parameters: &[],
        };
        let proto_req = SignRequest {
            payload,
            algorithm: AlgorithmId::Ed25519,
            key: SigningKey::Ed25519(&sk),
            keyid: None,
            append_missing_final_newline: false,
            output_form: OutputForm::Protobuf,
            algorithm_parameters: &[],
        };
        let y1 = sign_yaml(&SignYamlParams {
            payload,
            algorithm: AlgorithmId::Ed25519,
            key: SigningKey::Ed25519(&sk),
            keyid: None,
            append_missing_final_newline: false,
        })
        .unwrap();
        let y2 = match sign(&yaml_req) {
            SignOutcome::Success(s) => s.artifact,
            _ => panic!("expected success"),
        };
        assert_eq!(y1, y2);
        let p1 = sign_proto(&SignProtoParams {
            payload,
            algorithm: AlgorithmId::Ed25519,
            key: SigningKey::Ed25519(&sk),
            keyid: None,
            append_missing_final_newline: false,
        })
        .unwrap();
        let p2 = match sign(&proto_req) {
            SignOutcome::Success(s) => s.artifact,
            _ => panic!("expected success"),
        };
        assert_eq!(p1, p2);
    }

    #[tokio::test]
    async fn default_async_signer_matches_free_function() {
        let sk = EdSk::from_bytes(&[5u8; 32]);
        let payload = b"a: b\n";
        let req = SignRequest {
            payload,
            algorithm: AlgorithmId::Ed25519,
            key: SigningKey::Ed25519(&sk),
            keyid: Some("kid-d"),
            append_missing_final_newline: false,
            output_form: OutputForm::Yaml,
            algorithm_parameters: &[],
        };
        let direct = match sign(&req) {
            SignOutcome::Success(s) => s.artifact,
            _ => panic!("expected success via free fn"),
        };
        let via_async_trait = match AsyncSigner::sign(&DefaultAsyncSigner, &req).await {
            SignOutcome::Success(s) => s.artifact,
            _ => panic!("expected success via async trait"),
        };
        assert_eq!(direct, via_async_trait);
        assert_eq!(
            AsyncSigner::capabilities(&DefaultAsyncSigner)
                .supported_algorithms
                .len(),
            signer_capabilities().supported_algorithms.len()
        );
    }

    #[tokio::test]
    async fn async_caller_rng_future_is_send() {
        fn require_send<F: core::future::Future + Send>(future: F) -> F {
            future
        }

        let sk = P256Sk::from_slice(&[7u8; 32]).expect("valid test key");
        let req = p256_request(&sk);
        let mut rng = PatternRng::new(0x44);
        let future = AsyncSignerWithRng::sign_with_rng(&DefaultAsyncSigner, &req, &mut rng);
        assert!(matches!(
            require_send(future).await,
            SignOutcome::Success(_)
        ));
        assert_eq!(rng.calls, 1);
    }
}
