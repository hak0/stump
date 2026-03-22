use std::{
	path::Path,
	sync::{OnceLock, RwLock},
};

use crate::{config::StumpConfig, prisma};

static ACTIVE_BACKEND: OnceLock<RwLock<DatabaseBackend>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DatabaseBackend {
	Sqlite,
	Postgres,
}

impl Default for DatabaseBackend {
	fn default() -> Self {
		Self::Sqlite
	}
}

impl DatabaseBackend {
	pub fn from_url(url: &str) -> Self {
		let normalized = url.trim().to_ascii_lowercase();

		if normalized.starts_with("postgresql://") || normalized.starts_with("postgres://") {
			Self::Postgres
		} else {
			Self::Sqlite
		}
	}

	pub fn supports_pragmas(self) -> bool {
		matches!(self, Self::Sqlite)
	}

	pub fn sqlite(self) -> bool {
		matches!(self, Self::Sqlite)
	}

	pub fn postgres(self) -> bool {
		matches!(self, Self::Postgres)
	}
}

fn active_backend_lock() -> &'static RwLock<DatabaseBackend> {
	ACTIVE_BACKEND.get_or_init(|| RwLock::new(DatabaseBackend::default()))
}

pub fn set_active_backend(backend: DatabaseBackend) {
	if let Ok(mut lock) = active_backend_lock().write() {
		*lock = backend;
	}
}

pub fn get_active_backend() -> DatabaseBackend {
	active_backend_lock()
		.read()
		.map(|lock| *lock)
		.unwrap_or_default()
}

pub fn default_sqlite_url(config: &StumpConfig) -> String {
	let config_dir = config
		.get_config_dir()
		.to_str()
		.expect("Error parsing config directory")
		.to_string();

	if let Some(path) = config.db_path.clone() {
		format!("file:{path}/stump.db")
	} else if config.profile == "release" {
		format!("file:{config_dir}/stump.db")
	} else {
		format!("file:{}/prisma/dev.db", env!("CARGO_MANIFEST_DIR"))
	}
}

pub fn resolve_database_url(config: &StumpConfig) -> String {
	config
		.database_url
		.clone()
		.unwrap_or_else(|| default_sqlite_url(config))
}

/// Creates the [`prisma::PrismaClient`]. Will call `create_data_dir` as well
pub async fn create_client(config: &StumpConfig) -> prisma::PrismaClient {
	let database_url = resolve_database_url(config);

	tracing::trace!(?database_url, "Creating Prisma client");
	create_client_with_url(&database_url).await
}

pub async fn create_client_with_url(url: &str) -> prisma::PrismaClient {
	set_active_backend(DatabaseBackend::from_url(url));
	std::env::set_var("DATABASE_URL", url);

	prisma::new_client_with_url(url)
		.await
		.expect("Failed to create Prisma client")
}

pub async fn create_test_client() -> prisma::PrismaClient {
	let test_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("integration-tests");

	create_client_with_url(&format!("file:{}/test.db", test_dir.to_str().unwrap())).await
}
