//! HSMS message types, headers, control builders, channel, select, session I/O.
//!
//! Source: `Secs4Net.Hsms`.

mod builder;
mod channel;
mod error;
mod message;
mod message_type;
mod pass_through;
mod select;
mod session_io;
mod state;
mod status;
mod wire;

pub use builder::{
    build_data_message, build_data_reply, build_data_reply_from_header, build_deselect_request,
    build_deselect_request_gs, build_deselect_response, build_linktest_request,
    build_linktest_response, build_reject_request, build_select_request, build_select_request_gs,
    build_select_response, build_separate_request, build_separate_request_gs, SystemBytesCounter,
};
pub use channel::{
    read_frame, read_frame_t8, set_read_timeout, set_write_timeout, write_frame, HsmsTcpChannel,
};
pub use error::{Error, Result};
pub use message::HsmsMessage;
pub use message_type::HsmsMessageType;
pub use pass_through::HsmsPassThrough;
pub use select::{
    active_select, active_select_gs, passive_await_select_req, passive_select,
    passive_select_already_used, passive_select_gs, reply_select_actived, reply_select_status,
};
pub use session_io::{HsmsSessionIo, LinktestActivity, ReplyTimeoutClass, SelectedDispatch};
pub use state::HsmsCommunicateState;
pub use status::{DeselectStatus, RejectReason, SelectStatus};
pub use wire::{build_from_parts, decode_frame, encode_frame};

/// HSMS connection mode (ACTIVE / PASSIVE).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum HsmsConnectionMode {
    #[default]
    Passive,
    Active,
}
