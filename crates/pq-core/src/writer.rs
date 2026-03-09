use arrow::array::RecordBatch;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;
use std::fs::File;
use std::path::Path;

use crate::error::Result;

pub struct WriteOptions {
    pub compression: Compression,
    pub max_row_group_size: usize,
}

impl Default for WriteOptions {
    fn default() -> Self {
        Self {
            compression: Compression::ZSTD(Default::default()),
            max_row_group_size: 1_000_000,
        }
    }
}

pub fn write_batches(path: &Path, batches: &[RecordBatch], opts: &WriteOptions) -> Result<()> {
    if batches.is_empty() {
        return Err(crate::error::PqError::Other("No data to write".to_string()));
    }

    let schema = batches[0].schema();
    let file = File::create(path)?;

    let props = WriterProperties::builder()
        .set_compression(opts.compression)
        .set_max_row_group_size(opts.max_row_group_size)
        .build();

    let mut writer = ArrowWriter::try_new(file, schema, Some(props))?;

    for batch in batches {
        writer.write(batch)?;
    }

    writer.close()?;
    Ok(())
}
