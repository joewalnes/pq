use parquet::file::metadata::ParquetMetaData;
use parquet::file::statistics::Statistics;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ColumnStats {
    pub column_name: String,
    pub column_type: String,
    pub num_values: i64,
    pub null_count: Option<i64>,
    pub distinct_count: Option<u64>,
    pub min_value: Option<String>,
    pub max_value: Option<String>,
    pub compressed_size: i64,
    pub uncompressed_size: i64,
}

pub fn extract_column_stats(metadata: &ParquetMetaData) -> Vec<ColumnStats> {
    let schema = metadata.file_metadata().schema_descr();
    let num_columns = schema.num_columns();
    let mut stats: Vec<ColumnStats> = Vec::with_capacity(num_columns);

    for col_idx in 0..num_columns {
        let col_descr = schema.column(col_idx);
        let col_name = col_descr.path().string();
        let col_type = format!("{:?}", col_descr.physical_type());

        let mut total_values: i64 = 0;
        let mut total_nulls: i64 = 0;
        let mut total_distinct: Option<u64> = None;
        let mut overall_min: Option<String> = None;
        let mut overall_max: Option<String> = None;
        let mut compressed: i64 = 0;
        let mut uncompressed: i64 = 0;
        let mut has_null_stats = false;

        for rg in metadata.row_groups() {
            let col_chunk = rg.column(col_idx);
            total_values += col_chunk.num_values();
            compressed += col_chunk.compressed_size();
            uncompressed += col_chunk.uncompressed_size();

            if let Some(stat) = col_chunk.statistics() {
                if stat.null_count_opt().is_some() {
                    has_null_stats = true;
                    if let Some(nc) = stat.null_count_opt() {
                        total_nulls += nc as i64;
                    }
                }

                if let Some(dc) = stat.distinct_count_opt() {
                    total_distinct = Some(total_distinct.unwrap_or(0) + dc);
                }

                let min_str = stat_to_string_min(stat);
                let max_str = stat_to_string_max(stat);

                if let Some(ref m) = min_str {
                    if overall_min.is_none() || overall_min.as_ref().unwrap() > m {
                        overall_min = Some(m.clone());
                    }
                }
                if let Some(ref m) = max_str {
                    if overall_max.is_none() || overall_max.as_ref().unwrap() < m {
                        overall_max = Some(m.clone());
                    }
                }
            }
        }

        stats.push(ColumnStats {
            column_name: col_name,
            column_type: col_type,
            num_values: total_values,
            null_count: if has_null_stats {
                Some(total_nulls)
            } else {
                None
            },
            distinct_count: total_distinct,
            min_value: overall_min,
            max_value: overall_max,
            compressed_size: compressed,
            uncompressed_size: uncompressed,
        });
    }

    stats
}

/// Format raw bytes as a display-safe string.
/// Returns the UTF-8 string if all characters are printable, otherwise hex.
fn bytes_to_display(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) if s.chars().all(|c| !c.is_control()) => s.to_string(),
        _ => {
            let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02X}")).collect();
            format!("0x{}", hex.join(""))
        }
    }
}

fn stat_to_string_min(stat: &Statistics) -> Option<String> {
    Some(match stat {
        Statistics::Boolean(s) => format!("{}", s.min_opt()?),
        Statistics::Int32(s) => format!("{}", s.min_opt()?),
        Statistics::Int64(s) => format!("{}", s.min_opt()?),
        Statistics::Int96(s) => format!("{:?}", s.min_opt()?),
        Statistics::Float(s) => format!("{}", s.min_opt()?),
        Statistics::Double(s) => format!("{}", s.min_opt()?),
        Statistics::ByteArray(s) => bytes_to_display(s.min_bytes_opt()?),
        Statistics::FixedLenByteArray(s) => bytes_to_display(s.min_bytes_opt()?),
    })
}

fn stat_to_string_max(stat: &Statistics) -> Option<String> {
    Some(match stat {
        Statistics::Boolean(s) => format!("{}", s.max_opt()?),
        Statistics::Int32(s) => format!("{}", s.max_opt()?),
        Statistics::Int64(s) => format!("{}", s.max_opt()?),
        Statistics::Int96(s) => format!("{:?}", s.max_opt()?),
        Statistics::Float(s) => format!("{}", s.max_opt()?),
        Statistics::Double(s) => format!("{}", s.max_opt()?),
        Statistics::ByteArray(s) => bytes_to_display(s.max_bytes_opt()?),
        Statistics::FixedLenByteArray(s) => bytes_to_display(s.max_bytes_opt()?),
    })
}
