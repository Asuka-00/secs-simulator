//! GEM Stream-13 data-set GRANT helpers.
//!
//! Source: `AbstractGem.S13f12`.

use crate::hsms::HsmsMessage;
use crate::hsms_ss::HsmsSsCommunicator;
use crate::SecsMessage;

use super::ack::Grant;
use super::error::GemError;

/// S13F12 Data Set Object Multi-block Grant.
pub fn s13f12(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    grant: Grant,
) -> Result<bool, GemError> {
    if !primary.wbit() {
        return Ok(false);
    }
    comm.send_data_reply(primary, 13, 12, false, grant.secs2())?;
    Ok(true)
}
