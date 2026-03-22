use std::{error::Error, fs, path::PathBuf, process::Command};

fn generate_provider_schemas() -> Result<(), Box<dyn Error>> {
	let prisma_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../core/prisma");
	let shared_schema_path = prisma_dir.join("schema.prisma");
	let shared_schema = fs::read_to_string(&shared_schema_path)?;

	let sqlite_schema = shared_schema.replace(
		"  provider = \"sqlite\"",
		"  provider = \"sqlite\"",
	);
	let postgres_schema = shared_schema.replace(
		"  provider = \"sqlite\"",
		"  provider = \"postgresql\"",
	).replace(
		"  url      = \"file:dev.db\"",
		"  url      = env(\"DATABASE_URL\")",
	);

	fs::write(prisma_dir.join("schema.sqlite.prisma"), sqlite_schema)?;
	fs::write(prisma_dir.join("schema.postgresql.prisma"), postgres_schema)?;

	Ok(())
}

/// A simple program that executes various `cargo` commands to generate code
fn main() -> Result<(), Box<dyn Error>> {
	let args: Vec<String> = std::env::args().collect();
	let skip_prisma = args.get(1).map(|s| s == "--skip-prisma").unwrap_or(false);
	generate_provider_schemas()?;

	if skip_prisma {
		println!("Skipping prisma generation...");
	} else {
		// cargo prisma generate
		let command = Command::new("cargo")
			.args(["prisma", "generate"])
			.current_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../core"))
			.spawn()?
			.wait()?;
		assert!(command.success());
		println!("Prisma client has been generated successfully!");
	}

	let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../packages/sdk/src/types")
		.join("generated.ts");

	// cargo test --package stump_core --lib -- tests::codegen --ignored
	let command = Command::new("cargo")
		.args([
			"test",
			"--package",
			"stump_core",
			"--lib",
			"--",
			"tests::codegen",
			"--ignored",
		])
		.spawn()?
		.wait()?;
	assert!(command.success());
	assert!(path.exists());
	println!("core types have been generated successfully!");

	// cargo test --package stump_server  --bin stump_server -- routers::api::tests::codegen --ignored
	let command = Command::new("cargo")
		.args([
			"test",
			"--package",
			"stump_server",
			"--bin",
			"stump_server",
			"--",
			"routers::api::tests::codegen",
			"--ignored",
		])
		.spawn()?
		.wait()?;
	assert!(command.success());
	println!("server types have been generated successfully!");

	// cargo test --package stump_desktop -- tests::codegen --ignored
	let command = Command::new("cargo")
		.args([
			"test",
			"--package",
			"stump_desktop",
			"--",
			"tests::codegen",
			"--ignored",
		])
		.spawn()?
		.wait()?;
	assert!(command.success());
	println!("desktop types have been generated successfully!");

	println!("Code generation has been completed successfully!");

	Ok(())
}
