use parquet::file::metadata::ParquetMetaData;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct PhysicalLayout {
    pub num_row_groups: usize,
    pub row_groups: Vec<RowGroupLayout>,
}

#[derive(Debug, Serialize)]
pub struct RowGroupLayout {
    pub index: usize,
    pub num_rows: i64,
    pub total_byte_size: i64,
    pub columns: Vec<ColumnLayout>,
}

#[derive(Debug, Serialize)]
pub struct ColumnLayout {
    pub path: String,
    pub physical_type: String,
    pub compression: String,
    pub encodings: Vec<String>,
    pub compressed_size: i64,
    pub uncompressed_size: i64,
    pub num_values: i64,
    pub data_page_offset: i64,
    pub dictionary_page_offset: Option<i64>,
    pub has_bloom_filter: bool,
}

pub fn extract_physical_layout(metadata: &ParquetMetaData) -> PhysicalLayout {
    let row_groups = metadata
        .row_groups()
        .iter()
        .enumerate()
        .map(|(i, rg)| {
            let columns = rg
                .columns()
                .iter()
                .map(|col| ColumnLayout {
                    path: col.column_path().string(),
                    physical_type: format!("{:?}", col.column_type()),
                    compression: format!("{:?}", col.compression()),
                    encodings: col.encodings().iter().map(|e| format!("{e:?}")).collect(),
                    compressed_size: col.compressed_size(),
                    uncompressed_size: col.uncompressed_size(),
                    num_values: col.num_values(),
                    data_page_offset: col.data_page_offset(),
                    dictionary_page_offset: col.dictionary_page_offset(),
                    has_bloom_filter: col.bloom_filter_offset().is_some(),
                })
                .collect();

            RowGroupLayout {
                index: i,
                num_rows: rg.num_rows(),
                total_byte_size: rg.total_byte_size(),
                columns,
            }
        })
        .collect();

    PhysicalLayout {
        num_row_groups: metadata.num_row_groups(),
        row_groups,
    }
}
