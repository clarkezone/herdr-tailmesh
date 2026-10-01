pub mod client;
pub mod projection;
// Canonical control messages include large oneofs; the viewer never constructs
// those envelopes. Keep generated types faithful to the shared schema.
#[allow(clippy::large_enum_variant)]
pub mod pb {
    tonic::include_proto!("agentflow.v1");
}
