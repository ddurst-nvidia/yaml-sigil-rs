// SPDX-FileCopyrightText: Copyright 2026 NVIDIA CORPORATION & AFFILIATES
// SPDX-License-Identifier: Apache-2.0

#![no_std]
#![allow(dead_code)]

extern crate alloc;

use alloc::{string::String, vec::Vec};

use yaml_sigil_core::{
    AlgorithmId, CoreError, DecompositionOutcome, OuterConformance, PayloadInvariantError,
    ProtoArtifactView, ProtoOuterDecomposeOutcome, ProtobufWireDecodeAdvertisement,
    SCHEMA_V1ALPHA1, SignatureDocument, SignatureRanges, TIER_A_TOP_LEVEL_KEYS,
    YamlSignatureDocumentDuplicateKeyPolicy, YamlSignatureDocumentUnknownFieldPolicy,
    compose_proto_outer, decode_signature_carrier, decode_signed_yaml_artifact, decompose_artifact,
    decompose_proto_outer, encode_signed_yaml_artifact, parse_signature_document,
    serialize_signature_document, signature_document_top_level_keys, validate_payload_stream,
    view_signature_carrier, view_signed_yaml_artifact, yaml_unknown_field_policies,
};
use yaml_sigil_signing::{
    AsyncSigner, AsyncSignerWithRng, CryptoRngCore, DefaultAsyncSigner, DefaultSigner, OutputForm,
    SignError, SignInvocationError, SignOutcome, SignProtoParams, SignRequest, SignSuccess,
    SignYamlParams, Signer, SignerCapabilities, SignerWithRng, SigningKey, TranscodeError,
    proto_wire_to_signed_yaml_stream, sign, sign_proto, sign_proto_with_rng, sign_with_rng,
    sign_yaml, sign_yaml_with_rng, signed_yaml_stream_to_proto_wire, signer_capabilities,
    signer_capabilities_with_rng,
};
use yaml_sigil_transcription::{
    AbstractArtifact, AsyncTranscriber, ComposeOutcome, ComposeRequest, ComposeSuccess,
    DecomposeOutcome, DecomposeRequest, DecomposeResponse, DecomposeStructuralResult,
    DefaultAsyncTranscriber, DefaultTranscriber, Transcriber, TranscriberCapabilities,
    TranscriberError, TranscriberInvocationError, TranscriptionForm, compose, decompose,
    transcriber_capabilities,
};
use yaml_sigil_verification::{
    AdvertisedConformanceProfile, ArtifactForm, AsyncVerifier, DefaultAsyncVerifier,
    DefaultVerifier, InvocationError, PreVerifyOutcome, PreVerifyResponse, PublicKeys,
    UnverifiedSignature, Verifier, VerifierCapabilities, VerifierOptions, VerifierState,
    VerifyResult, can_pre_verify, pre_verify, pre_verify_proto, pre_verify_yaml,
    resolve_ed25519_verifying_key, resolve_p256_verifying_key, verifier_capabilities, verify,
    verify_from_pre_verify, verify_from_pre_verify_proto, verify_from_pre_verify_yaml,
    verify_proto, verify_with_metadata, verify_yaml,
};

fn assert_send<T: Send>(_value: T) {}

fn exercise_core() {
    let _ = SCHEMA_V1ALPHA1;
    let _ = TIER_A_TOP_LEVEL_KEYS;
    let _ = AlgorithmId::Ed25519;
    let _ = OuterConformance::Strict;
    let _ = ProtobufWireDecodeAdvertisement::UnprofiledStockDecoder;
    let _ = YamlSignatureDocumentDuplicateKeyPolicy::RejectedAtParse;
    let _ = YamlSignatureDocumentUnknownFieldPolicy::RejectedAtParse;
    let _ = yaml_unknown_field_policies();
    let _ = validate_payload_stream(&[]);
    let _ = decompose_artifact(&[]);
    let carrier = compose_proto_outer(&[], &[]);
    let _ = decompose_proto_outer(&carrier, OuterConformance::Strict);
    let _ = decode_signature_carrier(&[]);

    let document = SignatureDocument {
        schema: String::from(SCHEMA_V1ALPHA1),
        alg: String::new(),
        keyid: None,
        signature: String::new(),
    };
    let _ = parse_signature_document(&[]);
    let _ = serialize_signature_document(&document);
    let _ = signature_document_top_level_keys(&[]);

    let message = yaml_sigil_core::pb::SignedYamlArtifact::default();
    let wire = encode_signed_yaml_artifact(&message);
    let _ = decode_signed_yaml_artifact(&wire);
    let _ = view_signed_yaml_artifact(&message);
    let _ = view_signature_carrier(&[]);
    let _: Option<yaml_sigil_core::pb::Algorithm> = None;
    let _: Option<yaml_sigil_core::pb::YamlSigilSignature> = None;
    let _: Option<CoreError> = None;
    let _: Option<PayloadInvariantError> = None;
    let _: Option<DecompositionOutcome> = None;
    let _: Option<SignatureRanges> = None;
    let _: Option<ProtoOuterDecomposeOutcome> = None;
    let _: Option<ProtoArtifactView> = None;
}

fn exercise_transcription() {
    let compose_request = ComposeRequest {
        payload: &[],
        signature_carrier: &[],
        form: TranscriptionForm::Yaml,
    };
    let decompose_request = DecomposeRequest {
        artifact: &[],
        form: TranscriptionForm::Protobuf,
        outer_conformance: Some(OuterConformance::Strict),
    };
    let _ = transcriber_capabilities();
    let _ = compose(&compose_request);
    let _ = decompose(&decompose_request);
    let _ = Transcriber::capabilities(&DefaultTranscriber);
    let _ = Transcriber::compose(&DefaultTranscriber, &compose_request);
    let _ = Transcriber::decompose(&DefaultTranscriber, &decompose_request);
    assert_send(AsyncTranscriber::compose(
        &DefaultAsyncTranscriber,
        &compose_request,
    ));
    assert_send(AsyncTranscriber::decompose(
        &DefaultAsyncTranscriber,
        &decompose_request,
    ));

    let _ = AbstractArtifact {
        payload: Vec::new(),
        signature_carrier: Vec::new(),
    };
    let _ = ComposeOutcome::Success(ComposeSuccess {
        artifact: Vec::new(),
        form: TranscriptionForm::Yaml,
    });
    let _ = ComposeOutcome::Error(TranscriberError::InvalidPayloadBytes);
    let _ = DecomposeResponse::Structural(DecomposeStructuralResult {
        outcome: DecomposeOutcome::Ok,
        payload: None,
        signature_carrier: None,
        detail: None,
    });
    let _ = TranscriberInvocationError::InvalidOrUnsupportedForm;
    let _: Option<TranscriberCapabilities> = None;
}

fn exercise_signing(key: SigningKey<'_>, rng: &mut (dyn CryptoRngCore + Send)) {
    let request = SignRequest {
        payload: &[],
        algorithm: AlgorithmId::Ed25519,
        key,
        keyid: None,
        append_missing_final_newline: false,
        output_form: OutputForm::Yaml,
        algorithm_parameters: &[],
    };
    let yaml = SignYamlParams {
        payload: &[],
        algorithm: AlgorithmId::Ed25519,
        key,
        keyid: None,
        append_missing_final_newline: false,
    };
    let proto = SignProtoParams {
        payload: &[],
        algorithm: AlgorithmId::Ed25519,
        key,
        keyid: None,
        append_missing_final_newline: false,
    };

    let _ = signer_capabilities();
    let _ = signer_capabilities_with_rng();
    let _ = sign(&request);
    let _ = sign_with_rng(&request, rng);
    let _ = sign_yaml(&yaml);
    let _ = sign_yaml_with_rng(&yaml, rng);
    let _ = sign_proto(&proto);
    let _ = sign_proto_with_rng(&proto, rng);
    let _ = proto_wire_to_signed_yaml_stream(&[]);
    let _ = signed_yaml_stream_to_proto_wire(&[]);

    let signer: &dyn SignerWithRng<
        Ed25519SigningKey = <DefaultSigner as Signer>::Ed25519SigningKey,
        P256SigningKey = <DefaultSigner as Signer>::P256SigningKey,
    > = &DefaultSigner;
    let _ = Signer::capabilities(signer);
    let _ = Signer::sign(signer, &request);
    let _ = signer.capabilities_with_rng();
    let _ = signer.sign_with_rng(&request, rng);
    assert_send(AsyncSigner::sign(&DefaultAsyncSigner, &request));
    assert_send(AsyncSignerWithRng::sign_with_rng(
        &DefaultAsyncSigner,
        &request,
        rng,
    ));

    let _ = SignError::YamlSerialize(String::new());
    let _ = SignInvocationError::InvalidAlgorithmParameters;
    let _ = SignOutcome::Success(SignSuccess {
        artifact: Vec::new(),
        modified_payload: Vec::new(),
    });
    let _: Option<SignerCapabilities> = None;
    let _: Option<TranscodeError> = None;
}

fn exercise_verification(keys: &PublicKeys<'_>) {
    let options = VerifierOptions::default();
    let _ = verifier_capabilities();
    let _ = verify(&[], ArtifactForm::Yaml, keys, options.clone());
    let _ = verify_with_metadata(&[], ArtifactForm::Yaml, keys, options.clone(), true);
    let _ = verify_yaml(&[], keys, options.clone());
    let _ = verify_proto(&[], keys, options.clone());
    let pre = pre_verify(&[], ArtifactForm::Yaml, false, true);
    let _ = pre_verify_yaml(&[], false);
    let _ = pre_verify_proto(&[]);
    let _ = can_pre_verify(&[], ArtifactForm::Yaml, false);
    let _ = verify_from_pre_verify_yaml(&pre, keys, options.clone());
    let _ = verify_from_pre_verify_proto(&pre, keys, options.clone());
    let _ = verify_from_pre_verify(&pre, keys, options.clone());
    let _ = resolve_ed25519_verifying_key(&[]);
    let _ = resolve_p256_verifying_key(&[]);

    let _ = Verifier::capabilities(&DefaultVerifier);
    let _ = Verifier::pre_verify(&DefaultVerifier, &[], ArtifactForm::Yaml, false, true);
    let _ = Verifier::verify(
        &DefaultVerifier,
        &[],
        ArtifactForm::Yaml,
        keys,
        options.clone(),
    );
    assert_send(AsyncVerifier::pre_verify(
        &DefaultAsyncVerifier,
        &[],
        ArtifactForm::Yaml,
        false,
        true,
    ));
    assert_send(AsyncVerifier::verify(
        &DefaultAsyncVerifier,
        &[],
        ArtifactForm::Yaml,
        keys,
        options,
    ));

    let _ = AdvertisedConformanceProfile::Permissive;
    let _ = InvocationError::InvalidAlgorithmParameters;
    let _ = PreVerifyResponse {
        outcome: PreVerifyOutcome::MetadataParseFailure,
        form: ArtifactForm::Proto,
        unverified_payload_bytes: None,
        unverified_signature: None,
        parser_observations: Vec::new(),
    };
    let _ = UnverifiedSignature {
        algorithm: AlgorithmId::Ed25519,
        keyid: None,
        signature_octets: Vec::new(),
    };
    let _ = VerifierState::Verified {
        payload: Vec::new(),
        algorithm: AlgorithmId::Ed25519,
    };
    let _: Option<VerifierCapabilities> = None;
    let _: Option<VerifyResult> = None;
}
