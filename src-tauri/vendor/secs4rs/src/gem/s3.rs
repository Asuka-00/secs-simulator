//! GEM Stream-3 materials GRANT helpers.
//!
//! Source: `AbstractGem.S3f16`.

use crate::hsms::HsmsMessage;
use crate::hsms_ss::HsmsSsCommunicator;
use crate::SecsMessage;

use super::ack::Grant;
use super::error::GemError;

/// S3F16 Materials Multi-block Grant.
pub fn s3f16(
    comm: &HsmsSsCommunicator,
    primary: &HsmsMessage,
    grant: Grant,
) -> Result<bool, GemError> {
    if !primary.wbit() {
        return Ok(false);
    }
    comm.send_data_reply(primary, 3, 16, false, grant.secs2())?;
    Ok(true)
}
