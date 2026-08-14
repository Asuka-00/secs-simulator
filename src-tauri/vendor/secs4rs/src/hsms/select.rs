//! HSMS SELECT handshake on an already-connected channel.
//!
//! Source behavior:
//! - Active `Session.Select()` + MainTask: send SELECT.req, accept SUCCESS/ACTIVED
//! - Passive MainTask: T7 poll first primary; SELECT.req → SELECT.rsp SUCCESS
//!
//! Full transaction map / primary queue is deferred; this is the wire-level
//! select procedure used before the selected-message loop.

use std::time::Duration;

use super::builder::{build_select_request, build_select_request_gs, build_select_response};
use super::channel::HsmsTcpChannel;
use super::error::{Error, Result};
use super::message::HsmsMessage;
use super::message_type::HsmsMessageType;
use super::status::SelectStatus;

/// Map socket read timeout / would-block to a distinguishable HSMS timeout.
fn map_read_timeout(err: Error, as_t: Error) -> Error {
    match &err {
        Error::Io(m) if m.starts_with("timeout:") => as_t,
        _ => err,
    }
}

/// Active side: send SELECT.req, wait SELECT.rsp within `t6`.
///
/// Returns `Ok(true)` if status is SUCCESS or ACTIVED; `Ok(false)` for other
/// SELECT.rsp statuses (parity: Session.Select returns false, does not throw).
/// T6 → `Error::TimeoutT6`. Unexpected message type → `Ok(false)`.
pub fn active_select(
    channel: &mut HsmsTcpChannel,
    system_bytes: [u8; 4],
    t6: Duration,
) -> Result<bool> {
    let st = active_select_status(channel, build_select_request(system_bytes)?, t6)?;
    Ok(st == SelectStatus::Success || st == SelectStatus::Actived)
}

/// Active SELECT for HSMS-GS (device-bytes = session-id).
///
/// Returns the SELECT.rsp status (SUCCESS / ENTITY_* / …).
pub fn active_select_gs(
    channel: &mut HsmsTcpChannel,
    session_id: i32,
    system_bytes: [u8; 4],
    t6: Duration,
) -> Result<SelectStatus> {
    let req = build_select_request_gs(session_id, system_bytes)?;
    active_select_status(channel, req, t6)
}

fn active_select_status(
    channel: &mut HsmsTcpChannel,
    req: HsmsMessage,
    t6: Duration,
) -> Result<SelectStatus> {
    let key = req.system_bytes_key();
    channel.write_message(&req)?;

    channel.set_read_timeout(Some(t6))?;
    let rsp = match channel.read_message() {
        Ok(m) => m,
        Err(e) => {
            let _ = channel.set_read_timeout(None);
            return Err(map_read_timeout(e, Error::TimeoutT6));
        }
    };
    let _ = channel.set_read_timeout(None);

    if rsp.message_type() != HsmsMessageType::SelectRsp {
        return Ok(SelectStatus::NotSelectRsp);
    }
    if rsp.system_bytes_key() != key {
        return Ok(SelectStatus::NotSelectRsp);
    }
    Ok(SelectStatus::from_message(&rsp))
}

/// Passive GS: await SELECT.req, compute status via `decide`, reply.
///
/// Returns `(session_id, status)`.
pub fn passive_select_gs<F>(
    channel: &mut HsmsTcpChannel,
    t7: Duration,
    decide: F,
) -> Result<(i32, SelectStatus)>
where
    F: FnOnce(i32) -> SelectStatus,
{
    let initiate = passive_await_select_req(channel, t7)?;
    let sid = initiate.session_id();
    let status = decide(sid);
    reply_select_status(channel, &initiate, status)?;
    Ok((sid, status))
}

/// Wait for first message within `t7`; must be SELECT.req.
pub fn passive_await_select_req(channel: &mut HsmsTcpChannel, t7: Duration) -> Result<HsmsMessage> {
    channel.set_read_timeout(Some(t7))?;
    let initiate = match channel.read_message() {
        Ok(m) => m,
        Err(e) => {
            let _ = channel.set_read_timeout(None);
            return Err(map_read_timeout(e, Error::TimeoutT7));
        }
    };
    let _ = channel.set_read_timeout(None);

    if initiate.message_type() != HsmsMessageType::SelectReq {
        return Err(Error::PassiveNotSelectRequest);
    }
    Ok(initiate)
}

/// Reply SELECT.rsp with the given status (SUCCESS / ALREADY_USED / ACTIVED / …).
pub fn reply_select_status(
    channel: &mut HsmsTcpChannel,
    primary: &HsmsMessage,
    status: SelectStatus,
) -> Result<()> {
    let rsp = build_select_response(primary, status)?;
    channel.write_message(&rsp)
}

/// Passive side: wait for first message within `t7`; on SELECT.req reply SUCCESS.
///
/// Returns the initiating SELECT.req on success.
pub fn passive_select(channel: &mut HsmsTcpChannel, t7: Duration) -> Result<HsmsMessage> {
    let initiate = passive_await_select_req(channel, t7)?;
    reply_select_status(channel, &initiate, SelectStatus::Success)?;
    Ok(initiate)
}

/// Passive: SELECT.req → SELECT.rsp ALREADY_USED (session channel already set).
///
/// Parity: `SetChannel` failed → `HsmsMessageSelectStatus.ALREADY_USED`.
pub fn passive_select_already_used(channel: &mut HsmsTcpChannel, t7: Duration) -> Result<HsmsMessage> {
    let initiate = passive_await_select_req(channel, t7)?;
    reply_select_status(channel, &initiate, SelectStatus::AlreadyUsed)?;
    Ok(initiate)
}

/// Reply ACTIVED to a subsequent SELECT.req while already selected (passive path).
pub fn reply_select_actived(channel: &mut HsmsTcpChannel, req: &HsmsMessage) -> Result<()> {
    reply_select_status(channel, req, SelectStatus::Actived)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hsms::builder::SystemBytesCounter;
    use std::net::{TcpListener, TcpStream};
    use std::thread;
    use std::time::Duration;

    fn loopback_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = thread::spawn(move || TcpStream::connect(addr).unwrap());
        let (server, _) = listener.accept().unwrap();
        let client = client.join().unwrap();
        (server, client)
    }

    #[test]
    fn select_handshake_success() {
        let (server, client) = loopback_pair();
        let mut passive = HsmsTcpChannel::new(server);
        let mut active = HsmsTcpChannel::new(client);

        let sys = SystemBytesCounter::new().next(false, 10);

        let p = thread::spawn(move || {
            let init = passive_select(&mut passive, Duration::from_secs(2)).unwrap();
            assert_eq!(init.message_type(), HsmsMessageType::SelectReq);
            passive
        });

        let ok = active_select(&mut active, sys, Duration::from_secs(2)).unwrap();
        assert!(ok);

        let _passive = p.join().unwrap();
    }

    #[test]
    fn passive_rejects_non_select() {
        let (server, client) = loopback_pair();
        let mut passive = HsmsTcpChannel::new(server);
        let mut active = HsmsTcpChannel::new(client);

        // Send LINKTEST instead of SELECT
        let link = crate::hsms::builder::build_linktest_request([0, 0, 0, 1]).unwrap();
        active.write_message(&link).unwrap();

        let err = passive_select(&mut passive, Duration::from_secs(2)).unwrap_err();
        assert_eq!(err, Error::PassiveNotSelectRequest);
    }

    #[test]
    fn passive_t7_timeout() {
        let (server, _client) = loopback_pair();
        let mut passive = HsmsTcpChannel::new(server);
        let err = passive_select(&mut passive, Duration::from_millis(50)).unwrap_err();
        assert_eq!(err, Error::TimeoutT7);
    }

    #[test]
    fn passive_already_used_status() {
        let (server, client) = loopback_pair();
        let mut passive = HsmsTcpChannel::new(server);
        let mut active = HsmsTcpChannel::new(client);
        let sys = SystemBytesCounter::new().next(false, 10);

        let p = thread::spawn(move || {
            passive_select_already_used(&mut passive, Duration::from_secs(2)).unwrap();
        });
        let ok = active_select(&mut active, sys, Duration::from_secs(2)).unwrap();
        // AlreadyUsed is not Success/Actived → false
        assert!(!ok);
        p.join().unwrap();
    }

    #[test]
    fn active_select_gs_entity_unknown_status() {
        let (server, client) = loopback_pair();
        let mut passive = HsmsTcpChannel::new(server);
        let mut active = HsmsTcpChannel::new(client);
        let sys = SystemBytesCounter::new().next(false, 10);

        let p = thread::spawn(move || {
            let (sid, st) = passive_select_gs(&mut passive, Duration::from_secs(2), |_| {
                SelectStatus::EntityUnknown
            })
            .unwrap();
            assert_eq!(sid, 10);
            assert_eq!(st, SelectStatus::EntityUnknown);
        });
        let st = active_select_gs(&mut active, 10, sys, Duration::from_secs(2)).unwrap();
        assert_eq!(st, SelectStatus::EntityUnknown);
        p.join().unwrap();
    }
}
