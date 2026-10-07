pub mod client;
mod dismissal_store;
pub mod heartbeat;
pub mod projection;
pub mod summary;
// Canonical control messages include large oneofs; the viewer never constructs
// those envelopes. Keep generated types faithful to the shared schema.
#[allow(clippy::large_enum_variant)]
pub mod pb {
    tonic::include_proto!("agentflow.v1");
}
