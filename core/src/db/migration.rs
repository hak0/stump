use crate::{
	db::{get_active_backend, DatabaseBackend},
	error::CoreResult,
	prisma,
	CoreError,
};

pub async fn run_migrations(client: &prisma::PrismaClient) -> CoreResult<()> {
	tracing::info!("Running migrations...");

	if get_active_backend() == DatabaseBackend::Postgres {
		let mut builder = client._db_push();

		if std::env::var("FORCE_RESET_DB")
			.map(|v| v == "true")
			.unwrap_or(false)
		{
			tracing::debug!("Forcing PostgreSQL database reset...");
			builder = builder.force_reset();
		}

		builder
			.await
			.map_err(|e| CoreError::MigrationError(e.to_string()))?;

		tracing::info!("PostgreSQL database push completed!");
		return Ok(());
	}

	#[cfg(debug_assertions)]
	{
		let mut builder = client._db_push();

		if std::env::var("FORCE_RESET_DB")
			.map(|v| v == "true")
			.unwrap_or(false)
		{
			tracing::debug!("Forcing database reset...");
			builder = builder.force_reset();
		}

		tracing::debug!("Committing database push...");
		builder
			.await
			.map_err(|e| CoreError::MigrationError(e.to_string()))?;

		tracing::debug!("Database push completed!");
	}

	#[cfg(not(debug_assertions))]
	{
		client
			._migrate_deploy()
			.await
			.map_err(|e| CoreError::MigrationError(e.to_string()))?;

		tracing::info!("Database migration completed!");
	}

	Ok(())
}
