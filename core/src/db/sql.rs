use crate::db::get_active_backend;

pub fn sql_string_literal(value: &str) -> String {
	format!("'{}'", value.replace('\'', "''"))
}

pub fn sql_string_list(values: &[String]) -> String {
	values
		.iter()
		.map(|value| sql_string_literal(value))
		.collect::<Vec<_>>()
		.join(",")
}

pub fn coalesce_fn() -> &'static str {
	match get_active_backend() {
		crate::db::DatabaseBackend::Sqlite => "IFNULL",
		crate::db::DatabaseBackend::Postgres => "COALESCE",
	}
}
