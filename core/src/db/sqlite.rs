use std::str::FromStr;

use prisma_client_rust::raw;
use serde::{Deserialize, Serialize};

use crate::{
	db::{get_active_backend, DatabaseBackend},
	error::CoreResult,
	prisma::PrismaClient,
	CoreError,
};

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum JournalMode {
	#[serde(alias = "wal")]
	WAL,
	#[serde(alias = "delete")]
	DELETE,
}

impl Default for JournalMode {
	fn default() -> Self {
		Self::WAL
	}
}

impl AsRef<str> for JournalMode {
	fn as_ref(&self) -> &str {
		match self {
			Self::WAL => "WAL",
			Self::DELETE => "DELETE",
		}
	}
}

impl FromStr for JournalMode {
	type Err = String;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s.to_uppercase().as_str() {
			"WAL" => Ok(Self::WAL),
			"DELETE" => Ok(Self::DELETE),
			_ => Err(format!("Invalid or unsupported journal mode: {s}")),
		}
	}
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JournalModeQueryResult {
	pub journal_mode: JournalMode,
}

#[async_trait::async_trait]
pub trait DBPragma {
	async fn get_journal_mode(&self) -> CoreResult<JournalMode>;
	async fn set_journal_mode(&self, mode: JournalMode) -> CoreResult<JournalMode>;
}

#[async_trait::async_trait]
impl DBPragma for PrismaClient {
	async fn get_journal_mode(&self) -> CoreResult<JournalMode> {
		if get_active_backend() != DatabaseBackend::Sqlite {
			return Err(CoreError::BadRequest(
				"Journal mode is only supported for SQLite databases".to_string(),
			));
		}

		let result_vec = self
			._query_raw::<JournalModeQueryResult>(raw!("PRAGMA journal_mode;"))
			.exec()
			.await?;
		let result = result_vec.first();

		if let Some(record) = result {
			Ok(record.journal_mode)
		} else {
			tracing::warn!("No journal mode found! Defaulting to WAL assumption");
			Ok(JournalMode::default())
		}
	}

	async fn set_journal_mode(&self, mode: JournalMode) -> CoreResult<JournalMode> {
		if get_active_backend() != DatabaseBackend::Sqlite {
			return Err(CoreError::BadRequest(
				"Journal mode is only supported for SQLite databases".to_string(),
			));
		}

		let result_vec = self
			._query_raw::<JournalModeQueryResult>(raw!(&format!(
				"PRAGMA journal_mode={};",
				mode.as_ref()
			)))
			.exec()
			.await?;
		let record = result_vec.first().ok_or_else(|| {
			CoreError::InternalError("Journal mode failed to be set".to_string())
		})?;

		Ok(record.journal_mode)
	}
}
