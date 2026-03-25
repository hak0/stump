use std::{
	collections::HashMap,
	fs::File,
	io::Read,
	path::PathBuf,
	sync::{Arc, Condvar, Mutex, OnceLock},
	time::UNIX_EPOCH,
};
use tracing::{debug, error, trace};

use crate::{
	config::StumpConfig,
	db::entity::MediaMetadata,
	filesystem::{
		content_type::ContentType,
		error::FileError,
		hash,
		media::{
			process::{FileProcessor, FileProcessorOptions, ProcessedFile},
			utils::metadata_from_buf,
		},
		FileParts, PathUtils, ProcessedFileHashes,
	},
};

/// A file processor for ZIP files.
pub struct ZipProcessor;

#[derive(Clone, PartialEq, Eq)]
struct ArchiveSignature {
	modified_at_unix_nanos: Option<u128>,
	size_bytes: u64,
}

#[derive(Clone)]
struct IndexedZipEntry {
	archive_index: usize,
	content_type: ContentType,
}

struct ZipPageIndex {
	entries: Vec<IndexedZipEntry>,
}

enum ZipPageIndexState {
	Building,
	Ready(Arc<ZipPageIndex>),
}

struct ZipPageIndexCacheEntry {
	signature: ArchiveSignature,
	state: Mutex<ZipPageIndexState>,
	notify: Condvar,
}

type ZipPageIndexCache = HashMap<String, Arc<ZipPageIndexCacheEntry>>;

fn zip_page_index_cache() -> &'static Mutex<ZipPageIndexCache> {
	static CACHE: OnceLock<Mutex<ZipPageIndexCache>> = OnceLock::new();
	CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn archive_signature(path: &str) -> Result<ArchiveSignature, FileError> {
	let metadata = std::fs::metadata(path)?;
	let modified_at_unix_nanos = metadata
		.modified()
		.ok()
		.and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
		.map(|duration| duration.as_nanos());

	Ok(ArchiveSignature {
		modified_at_unix_nanos,
		size_bytes: metadata.len(),
	})
}

fn build_page_index(path: &str) -> Result<ZipPageIndex, FileError> {
	let zip_file = File::open(path)?;
	let mut archive = zip::ZipArchive::new(zip_file)?;

	if archive.is_empty() {
		error!(path, "Empty zip file");
		return Err(FileError::ArchiveEmptyError);
	}

	let mut sortable_entries = Vec::new();

	for archive_index in 0..archive.len() {
		let file = archive.by_index(archive_index)?;

		if file.is_dir() {
			continue;
		}

		let path_buf = file.enclosed_name().unwrap_or_else(|| {
			tracing::warn!("Failed to get enclosed name for zip entry");
			PathBuf::from(file.name())
		});
		let path = path_buf.as_path();

		if path.is_hidden_file() {
			trace!(path = ?path_buf, "Skipping hidden file");
			continue;
		}

		let content_type = path.naive_content_type();
		if content_type.is_image() {
			sortable_entries.push((path.to_string_lossy().to_string(), IndexedZipEntry {
				archive_index,
				content_type,
			}));
		}
	}

	sortable_entries.sort_by(|(left_name, _), (right_name, _)| {
		alphanumeric_sort::compare_str(left_name, right_name)
	});

	let entries = sortable_entries
		.into_iter()
		.map(|(_, entry)| entry)
		.collect::<Vec<_>>();

	Ok(ZipPageIndex { entries })
}

fn get_or_build_page_index(path: &str) -> Result<Arc<ZipPageIndex>, FileError> {
	let signature = archive_signature(path)?;
	let path_key = path.to_string();

	let (cache_entry, is_builder) = {
		let mut cache = zip_page_index_cache()
			.lock()
			.map_err(|_| FileError::UnknownError("ZIP page index cache poisoned".to_string()))?;

		match cache.get(&path_key) {
			Some(existing) if existing.signature == signature => (existing.clone(), false),
			_ => {
				let entry = Arc::new(ZipPageIndexCacheEntry {
					signature,
					state: Mutex::new(ZipPageIndexState::Building),
					notify: Condvar::new(),
				});
				cache.insert(path_key.clone(), entry.clone());
				(entry, true)
			},
		}
	};

	if is_builder {
		match build_page_index(path) {
			Ok(index) => {
				let ready_index = Arc::new(index);
				let mut state = cache_entry.state.lock().map_err(|_| {
					FileError::UnknownError("ZIP page index cache entry poisoned".to_string())
				})?;
				*state = ZipPageIndexState::Ready(ready_index.clone());
				cache_entry.notify.notify_all();
				Ok(ready_index)
			},
			Err(error) => {
				let mut cache = zip_page_index_cache().lock().map_err(|_| {
					FileError::UnknownError("ZIP page index cache poisoned".to_string())
				})?;
				if cache
					.get(&path_key)
					.is_some_and(|current| Arc::ptr_eq(current, &cache_entry))
				{
					cache.remove(&path_key);
				}

				cache_entry.notify.notify_all();
				Err(error)
			},
		}
	} else {
		let mut state = cache_entry.state.lock().map_err(|_| {
			FileError::UnknownError("ZIP page index cache entry poisoned".to_string())
		})?;

		loop {
			match &*state {
				ZipPageIndexState::Ready(index) => return Ok(index.clone()),
				ZipPageIndexState::Building => {
					state = cache_entry.notify.wait(state).map_err(|_| {
						FileError::UnknownError("ZIP page index cache entry poisoned".to_string())
					})?;
				},
			}
		}
	}
}

impl FileProcessor for ZipProcessor {
	fn get_sample_size(path: &str) -> Result<u64, FileError> {
		let zip_file = File::open(path)?;
		let mut archive = zip::ZipArchive::new(zip_file)?;

		let mut sample_size = 0;

		for i in 0..archive.len() {
			if i > 5 {
				break;
			}

			if let Ok(file) = archive.by_index(i) {
				sample_size += file.size();
			}
		}

		// TODO: sample size needs to be > 0...
		Ok(sample_size)
	}

	fn generate_stump_hash(path: &str) -> Option<String> {
		let sample_result = Self::get_sample_size(path);

		if let Ok(sample) = sample_result {
			match hash::generate(path, sample) {
				Ok(digest) => Some(digest),
				Err(e) => {
					debug!(error = ?e, path, "Failed to digest zip file");

					None
				},
			}
		} else {
			None
		}
	}

	fn generate_hashes(
		path: &str,
		FileProcessorOptions {
			generate_file_hashes,
			// generate_koreader_hashes,
			..
		}: FileProcessorOptions,
	) -> Result<ProcessedFileHashes, FileError> {
		let hash = generate_file_hashes
			.then(|| ZipProcessor::generate_stump_hash(path))
			.flatten();
		// TODO(koreader): Do we want to hash ZIP files?
		// let koreader_hash = generate_koreader_hashes
		// 	.then(|| generate_koreader_hash(path))
		// 	.transpose()?;

		Ok(ProcessedFileHashes {
			hash,
			koreader_hash: None,
		})
	}

	fn process_metadata(path: &str) -> Result<Option<MediaMetadata>, FileError> {
		let zip_file = File::open(path)?;
		let mut archive = zip::ZipArchive::new(zip_file)?;

		let mut metadata = None;

		for i in 0..archive.len() {
			let mut file = archive.by_index(i)?;

			if file.is_dir() {
				trace!("Skipping directory");
				continue;
			}

			let path_buf = file.enclosed_name().unwrap_or_else(|| {
				tracing::warn!("Failed to get enclosed name for zip entry");
				PathBuf::from(file.name())
			});
			let path = path_buf.as_path();

			if path.is_hidden_file() {
				trace!(path = ?path, "Skipping hidden file");
				continue;
			}

			let FileParts { file_name, .. } = path.file_parts();

			if file_name == "ComicInfo.xml" {
				trace!("Found ComicInfo.xml");
				let mut contents = Vec::new();
				file.read_to_end(&mut contents)?;
				let contents = String::from_utf8_lossy(&contents).to_string();
				trace!(contents_len = contents.len(), "Read ComicInfo.xml");
				metadata = metadata_from_buf(&contents);
				break;
			}
		}

		Ok(metadata)
	}

	fn process(
		path: &str,
		options: FileProcessorOptions,
		_: &StumpConfig,
	) -> Result<ProcessedFile, FileError> {
		let zip_file = File::open(path)?;
		let mut archive = zip::ZipArchive::new(zip_file)?;

		let mut metadata = None;
		let mut pages = 0;

		let ProcessedFileHashes {
			hash,
			koreader_hash,
		} = Self::generate_hashes(path, options)?;

		for i in 0..archive.len() {
			let mut file = archive.by_index(i)?;

			if file.is_dir() {
				trace!("Skipping directory");
				continue;
			}

			let path_buf = file.enclosed_name().unwrap_or_else(|| {
				tracing::warn!("Failed to get enclosed name for zip entry");
				PathBuf::from(file.name())
			});
			let path = path_buf.as_path();

			if path.is_hidden_file() {
				trace!(path = ?path, "Skipping hidden file");
				continue;
			}

			let content_type = path.naive_content_type();
			let FileParts { file_name, .. } = path.file_parts();

			if file_name == "ComicInfo.xml" && options.process_metadata {
				trace!("Found ComicInfo.xml");
				let mut contents = Vec::new();
				file.read_to_end(&mut contents)?;
				let contents = String::from_utf8_lossy(&contents).to_string();
				trace!(contents_len = contents.len(), "Read ComicInfo.xml");
				metadata = metadata_from_buf(&contents);
			} else if content_type.is_image() {
				pages += 1;
			}
		}

		Ok(ProcessedFile {
			path: PathBuf::from(path),
			hash,
			koreader_hash,
			metadata,
			pages,
		})
	}

	fn get_page(
		path: &str,
		page: i32,
		_: &StumpConfig,
	) -> Result<(ContentType, Vec<u8>), FileError> {
		let zip_file = File::open(path)?;
		let mut archive = zip::ZipArchive::new(&zip_file)?;
		let page_index = get_or_build_page_index(path)?;

		let target_entry = page_index
			.entries
			.get((page - 1) as usize)
			.ok_or(FileError::NoImageError)?;
		let mut file = archive.by_index(target_entry.archive_index)?;
		let mut contents = Vec::new();
		file.read_to_end(&mut contents)?;
		trace!(page, contents_len = contents.len(), "Read zip entry");

		Ok((target_entry.content_type, contents))
	}

	fn get_page_count(path: &str, _: &StumpConfig) -> Result<i32, FileError> {
		Ok(get_or_build_page_index(path)?.entries.len() as i32)
	}

	fn get_page_content_types(
		path: &str,
		pages: Vec<i32>,
	) -> Result<HashMap<i32, ContentType>, FileError> {
		let page_index = get_or_build_page_index(path)?;

		let mut content_types = HashMap::new();
		for page in pages {
			if let Some(entry) = page_index.entries.get((page - 1) as usize) {
				content_types.insert(page, entry.content_type);
			}
		}

		Ok(content_types)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::filesystem::{
		media::tests::{
			get_nested_macos_compressed_cbz_path, get_test_cbz_path, get_test_zip_path,
		},
		tests::get_test_complex_zip_path,
	};

	#[test]
	fn test_process() {
		let path = get_test_zip_path();
		let config = StumpConfig::debug();

		let processed_file = ZipProcessor::process(
			&path,
			FileProcessorOptions {
				convert_rar_to_zip: false,
				delete_conversion_source: false,
				..Default::default()
			},
			&config,
		);
		assert!(processed_file.is_ok());
	}

	#[test]
	fn test_process_cbz() {
		let path = get_test_cbz_path();
		let config = StumpConfig::debug();

		let processed_file = ZipProcessor::process(
			&path,
			FileProcessorOptions {
				convert_rar_to_zip: false,
				delete_conversion_source: false,
				..Default::default()
			},
			&config,
		);
		assert!(processed_file.is_ok());
	}

	#[test]
	fn test_process_nested_cbz() {
		let path = get_nested_macos_compressed_cbz_path();
		let config = StumpConfig::debug();

		let processed_file = ZipProcessor::process(
			&path,
			FileProcessorOptions {
				convert_rar_to_zip: false,
				delete_conversion_source: false,
				..Default::default()
			},
			&config,
		);
		assert!(processed_file.is_ok());
		assert_eq!(processed_file.unwrap().pages, 3);
	}

	#[test]
	fn test_get_page_cbz() {
		// Note: This doesn't work with the other test book, because it has no pages.
		let path = get_test_cbz_path();
		let config = StumpConfig::debug();

		let page = ZipProcessor::get_page(&path, 1, &config);
		assert!(page.is_ok());
	}

	#[test]
	fn test_get_page_nested_cbz() {
		let path = get_nested_macos_compressed_cbz_path();

		let (content_type, buf) = ZipProcessor::get_page(&path, 1, &StumpConfig::debug())
			.expect("Failed to get page");
		assert_eq!(content_type.mime_type(), "image/jpeg");
		// Note: this is known and expected to be 96623 bytes.
		assert_eq!(buf.len(), 96623);
	}

	#[test]
	fn test_get_page_content_types() {
		let path = get_test_zip_path();

		let content_types = ZipProcessor::get_page_content_types(&path, vec![1]);
		assert!(content_types.is_ok());
	}

	#[test]
	fn test_get_page_content_types_cbz() {
		let path = get_test_cbz_path();

		let content_types =
			ZipProcessor::get_page_content_types(&path, vec![1, 2, 3, 4, 5]);
		assert!(content_types.is_ok());
	}

	#[test]
	fn test_get_page_content_types_nested_cbz() {
		let path = get_nested_macos_compressed_cbz_path();

		let content_types = ZipProcessor::get_page_content_types(&path, vec![1, 2, 3])
			.expect("Failed to get page content types");
		assert_eq!(content_types.len(), 3);
		assert!(content_types
			.values()
			.all(|ct| ct.mime_type() == "image/jpeg"));
	}

	#[test]
	fn test_zip_with_complex_file_tree() {
		let path = get_test_complex_zip_path();

		let config = StumpConfig::debug();
		let processed_file = ZipProcessor::process(
			&path,
			FileProcessorOptions {
				process_metadata: true,
				..Default::default()
			},
			&config,
		)
		.expect("Failed to process ZIP file");

		// See https://github.com/stumpapp/stump/issues/641
		assert!(processed_file.metadata.is_some());
	}
}
