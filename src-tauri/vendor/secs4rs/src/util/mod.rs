//! Entity adapters and misc utilities (as needed by protocol paths).
//!
//! Source: `Secs4Net.Util`.

mod entity;
mod entity_adapter;
mod entity_sender;
mod entity_ss;

pub use entity::HsmsSsEntity;
pub use entity_adapter::{
    equals_device_id_or_session_id, s9_mhead_body, EntityEventAdapter, EntityMessageListener,
    EntityReplySink, EntityStateListener,
};
pub use entity_sender::EntityMessageSender;
pub use entity_ss::HsmsSsEntitySink;
