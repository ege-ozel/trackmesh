//! Transfer session states. SQLite text and JSON labels remain compatible with schema v2.
use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TransferState {
    Queued,
    Receiving,
    Paused,
    Applying,
    RecoveryRequired,
    Complete,
}

impl TransferState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "QUEUED",
            Self::Receiving => "RECEIVING",
            Self::Paused => "PAUSED",
            Self::Applying => "APPLYING",
            Self::RecoveryRequired => "RECOVERY_REQUIRED",
            Self::Complete => "COMPLETE",
        }
    }

    pub const fn requires_recovery(self) -> bool {
        matches!(self, Self::Applying | Self::RecoveryRequired)
    }
}

impl ToSql for TransferState {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Borrowed(ValueRef::Text(
            self.as_str().as_bytes(),
        )))
    }
}

impl FromSql for TransferState {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        match value.as_str()? {
            "QUEUED" => Ok(Self::Queued),
            "RECEIVING" => Ok(Self::Receiving),
            "PAUSED" => Ok(Self::Paused),
            "APPLYING" => Ok(Self::Applying),
            "RECOVERY_REQUIRED" => Ok(Self::RecoveryRequired),
            "COMPLETE" => Ok(Self::Complete),
            unknown => Err(FromSqlError::Other(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown TrackMesh transfer state: {unknown}"),
            )))),
        }
    }
}
