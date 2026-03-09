use parquet::file::metadata::ParquetMetaData;
use parquet::file::reader::{FileReader, SerializedFileReader};
use serde::Serialize;
use std::fs::File;
use std::path::Path;

use crate::error::{PqError, Result};

#[derive(Debug, Serialize)]
pub struct FileMetadata {
    pub path: String,
    pub file_size: u64,
    pub num_rows: i64,
    pub num_row_groups: usize,
    pub num_columns: usize,
    pub created_by: Option<String>,
    pub format_version: String,
    pub schema_name: String,
    pub key_value_metadata: Vec<KeyValuePair>,
    pub compression: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct KeyValuePair {
    pub key: String,
    pub value: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RowGroupInfo {
    pub index: usize,
    pub num_rows: i64,
    pub total_byte_size: i64,
    pub columns: Vec<ColumnChunkInfo>,
}

#[derive(Debug, Serialize)]
pub struct ColumnChunkInfo {
    pub column_path: String,
    pub compression: String,
    pub encodings: Vec<String>,
    pub total_compressed_size: i64,
    pub total_uncompressed_size: i64,
    pub num_values: i64,
    pub data_page_offset: i64,
    pub dictionary_page_offset: Option<i64>,
}

pub fn read_metadata(path: &Path) -> Result<ParquetMetaData> {
    let file = File::open(path).map_err(|e| PqError::FileOpen {
        path: path.display().to_string(),
        source: e,
    })?;
    let reader = SerializedFileReader::new(file).map_err(|e| PqError::ParquetRead {
        path: path.display().to_string(),
        source: e,
    })?;
    Ok(reader.metadata().clone())
}

pub fn extract_file_metadata(path: &Path) -> Result<FileMetadata> {
    let metadata = read_metadata(path)?;
    let file_meta = metadata.file_metadata();
    let file_size = std::fs::metadata(path)
        .map_err(|e| PqError::FileOpen {
            path: path.display().to_string(),
            source: e,
        })?
        .len();

    let mut compressions: Vec<String> = Vec::new();
    for rg in metadata.row_groups() {
        for col in rg.columns() {
            let comp = format!("{:?}", col.compression());
            if !compressions.contains(&comp) {
                compressions.push(comp);
            }
        }
    }

    let key_value_metadata = file_meta
        .key_value_metadata()
        .map(|kvs| {
            kvs.iter()
                .map(|kv| KeyValuePair {
                    key: kv.key.clone(),
                    value: kv.value.clone(),
                })
                .collect()
        })
        .unwrap_or_default();

    let version = match file_meta.version() {
        1 => "1.0".to_string(),
        2 => "2.6".to_string(),
        v => format!("{v}"),
    };

    Ok(FileMetadata {
        path: path.display().to_string(),
        file_size,
        num_rows: file_meta.num_rows(),
        num_row_groups: metadata.num_row_groups(),
        num_columns: file_meta.schema_descr().num_columns(),
        created_by: file_meta.created_by().map(|s| s.to_string()),
        format_version: version,
        schema_name: file_meta.schema_descr().name().to_string(),
        key_value_metadata,
        compression: compressions,
    })
}

pub fn extract_row_groups(path: &Path) -> Result<Vec<RowGroupInfo>> {
    let metadata = read_metadata(path)?;
    let mut row_groups = Vec::new();

    for (i, rg) in metadata.row_groups().iter().enumerate() {
        let mut columns = Vec::new();
        for col in rg.columns() {
            columns.push(ColumnChunkInfo {
                column_path: col.column_path().string(),
                compression: format!("{:?}", col.compression()),
                encodings: col.encodings().iter().map(|e| format!("{e:?}")).collect(),
                total_compressed_size: col.compressed_size(),
                total_uncompressed_size: col.uncompressed_size(),
                num_values: col.num_values(),
                data_page_offset: col.data_page_offset(),
                dictionary_page_offset: col.dictionary_page_offset(),
            });
        }

        row_groups.push(RowGroupInfo {
            index: i,
            num_rows: rg.num_rows(),
            total_byte_size: rg.total_byte_size(),
            columns,
        });
    }

    Ok(row_groups)
}
