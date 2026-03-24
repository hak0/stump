use std::collections::HashMap;

use prisma_client_rust::{raw, PrismaValue, Raw};
use serde::Deserialize;
use stump_core::{
	db::{
		entity::{macros::media_id_select, Media},
		query::pagination::Pagination,
		sql_string_list,
	},
	prisma::{media, PrismaClient},
};

use crate::errors::{APIError, APIResult};

#[derive(Deserialize)]
struct MediaIdRow {
	id: String,
}

pub(crate) fn is_random_ordering(order_by: &str) -> bool {
	order_by.eq_ignore_ascii_case("random")
}

pub(crate) async fn get_randomized_media_ids(
	client: &PrismaClient,
	where_conditions: Vec<media::WhereParam>,
	pagination: &Pagination,
) -> APIResult<(Vec<String>, Option<i64>)> {
	if pagination.is_cursor() {
		return Err(APIError::BadRequest(
			"Random ordering does not support cursor pagination".to_string(),
		));
	}

	let candidate_ids = client
		.media()
		.find_many(where_conditions)
		.select(media_id_select::select())
		.exec()
		.await?
		.into_iter()
		.map(|media| media.id)
		.collect::<Vec<_>>();

	if candidate_ids.is_empty() {
		let count = (!pagination.is_unpaged()).then_some(0);
		return Ok((vec![], count));
	}

	let randomized_ids = client
		._query_raw::<MediaIdRow>(randomized_media_ids_query(&candidate_ids, pagination))
		.exec()
		.await?
		.into_iter()
		.map(|row| row.id)
		.collect::<Vec<_>>();

	let count = (!pagination.is_unpaged()).then_some(candidate_ids.len() as i64);

	Ok((randomized_ids, count))
}

pub(crate) fn sort_media_by_ids(media: Vec<Media>, ids: &[String]) -> Vec<Media> {
	let mut media_by_id = media
		.into_iter()
		.map(|media| (media.id.clone(), media))
		.collect::<HashMap<_, _>>();

	ids.iter()
		.filter_map(|id| media_by_id.remove(id))
		.collect::<Vec<_>>()
}

fn randomized_media_ids_query(ids: &[String], pagination: &Pagination) -> Raw {
	let id_list = sql_string_list(ids);

	match pagination {
		Pagination::Page(page_query) => {
			let (skip, take) = page_query.get_skip_take();
			raw!(
				&format!(
					r#"
					SELECT id FROM media
					WHERE id IN ({id_list})
					ORDER BY RANDOM()
					LIMIT {{}} OFFSET {{}}
					"#
				),
				PrismaValue::Int(take),
				PrismaValue::Int(skip)
			)
		},
		Pagination::None => raw!(
			&format!(
				r#"
				SELECT id FROM media
				WHERE id IN ({id_list})
				ORDER BY RANDOM()
				"#
			)
		),
		Pagination::Cursor(_) => unreachable!(),
	}
}
