use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;

use arrow::array::RecordBatch;
use arrow::datatypes::Schema;
use arrow::util::display::ArrayFormatter;

use pq_core::reader::{open_batches, ReadOptions};

pub const PAGE_SIZE: usize = 500;
const MAX_PAGES: usize = 10;

pub struct Page {
    pub rows: Vec<Vec<String>>,
    pub batches: Vec<RecordBatch>,
}

struct FetchRequest {
    page_index: usize,
    offset: usize,
    limit: usize,
}

enum FetchResult {
    PageReady { page_index: usize, page: Page },
    FetchError { page_index: usize },
}

pub struct PageCache {
    pages: HashMap<usize, Page>,
    lru_order: Vec<usize>,
    pending: HashSet<usize>,
    request_tx: Sender<FetchRequest>,
    result_rx: Receiver<FetchResult>,
    pub total_rows: usize,
    pub schema: Arc<Schema>,
}

impl PageCache {
    pub fn new(
        location: String,
        schema: Arc<Schema>,
        total_rows: usize,
        first_page: Option<Page>,
    ) -> Self {
        let (request_tx, request_rx) = mpsc::channel::<FetchRequest>();
        let (result_tx, result_rx) = mpsc::channel::<FetchResult>();

        // Spawn background fetch thread
        std::thread::spawn(move || {
            while let Ok(req) = request_rx.recv() {
                let opts = ReadOptions {
                    columns: None,
                    limit: Some(req.limit),
                    offset: Some(req.offset),
                    batch_size: 8192,
                };
                match open_batches(&location, &opts) {
                    Ok((batches, _schema)) => {
                        let rows = format_batches_to_strings(&batches);
                        let page = Page { rows, batches };
                        if result_tx
                            .send(FetchResult::PageReady {
                                page_index: req.page_index,
                                page,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(_) => {
                        if result_tx
                            .send(FetchResult::FetchError {
                                page_index: req.page_index,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        });

        let mut pages = HashMap::new();
        let mut lru_order = Vec::new();
        if let Some(page) = first_page {
            pages.insert(0, page);
            lru_order.push(0);
        }

        Self {
            pages,
            lru_order,
            pending: HashSet::new(),
            request_tx,
            result_rx,
            total_rows,
            schema,
        }
    }

    /// Get a formatted row by global index.
    pub fn get_row(&self, row_idx: usize) -> Option<&Vec<String>> {
        if row_idx >= self.total_rows {
            return None;
        }
        let page_idx = row_idx / PAGE_SIZE;
        let row_in_page = row_idx % PAGE_SIZE;
        self.pages.get(&page_idx)?.rows.get(row_in_page)
    }

    /// Get the batch and row-within-batch for a global row index (for detail panel).
    pub fn get_batch_row(&self, row_idx: usize) -> Option<(&RecordBatch, usize)> {
        if row_idx >= self.total_rows {
            return None;
        }
        let page_idx = row_idx / PAGE_SIZE;
        let row_in_page = row_idx % PAGE_SIZE;
        let page = self.pages.get(&page_idx)?;
        let mut offset = 0;
        for batch in &page.batches {
            let n = batch.num_rows();
            if row_in_page < offset + n {
                return Some((batch, row_in_page - offset));
            }
            offset += n;
        }
        None
    }

    /// Check if any fetches are in-flight.
    pub fn is_loading(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Drain completed fetches from the background thread.
    pub fn poll_fetches(&mut self) {
        while let Ok(result) = self.result_rx.try_recv() {
            match result {
                FetchResult::PageReady { page_index, page } => {
                    self.pending.remove(&page_index);
                    self.insert_page(page_index, page);
                }
                FetchResult::FetchError { page_index } => {
                    self.pending.remove(&page_index);
                }
            }
        }
    }

    /// Request the page containing `row_idx` plus 1 page ahead and behind.
    pub fn ensure_pages_around(&mut self, row_idx: usize) {
        if self.total_rows == 0 {
            return;
        }
        let page_idx = row_idx / PAGE_SIZE;
        let max_page = (self.total_rows - 1) / PAGE_SIZE;
        let start = page_idx.saturating_sub(1);
        let end = (page_idx + 1).min(max_page);

        for idx in start..=end {
            self.touch_lru(idx);
            self.request_page(idx);
        }
    }

    fn request_page(&mut self, page_idx: usize) {
        if self.pages.contains_key(&page_idx) || self.pending.contains(&page_idx) {
            return;
        }
        let offset = page_idx * PAGE_SIZE;
        if offset >= self.total_rows {
            return;
        }
        let limit = PAGE_SIZE.min(self.total_rows - offset);
        self.pending.insert(page_idx);
        // If send fails, the background thread has exited — ignore
        let _ = self.request_tx.send(FetchRequest {
            page_index: page_idx,
            offset,
            limit,
        });
    }

    fn insert_page(&mut self, page_idx: usize, page: Page) {
        // Evict if over capacity
        while self.pages.len() >= MAX_PAGES {
            if let Some(evict_idx) = self.lru_order.first().copied() {
                self.lru_order.remove(0);
                self.pages.remove(&evict_idx);
            } else {
                break;
            }
        }
        self.pages.insert(page_idx, page);
        self.touch_lru(page_idx);
    }

    fn touch_lru(&mut self, page_idx: usize) {
        if let Some(pos) = self.lru_order.iter().position(|&x| x == page_idx) {
            self.lru_order.remove(pos);
        }
        self.lru_order.push(page_idx);
    }
}

/// Format record batches into rows of strings.
///
/// One `ArrayFormatter` is built per *column per batch*, not per cell: the
/// previous version called `ArrayFormatter::try_new` inside the row loop, so
/// a batch of 500 rows x N columns built 500x as many formatters as it
/// needed to. `ArrayFormatter::try_new` inspects the array's data type and
/// the format options to pick a formatting strategy, which is exactly the
/// same work every time for a given (array, options) pair within one batch —
/// building it once per column and reusing it for every row is a pure
/// hoist, not a behavior change: a column whose formatter fails to construct
/// still renders every one of its cells as `"<error>"`, same as before.
pub fn format_batches_to_strings(batches: &[RecordBatch]) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    for batch in batches {
        let formatters: Vec<Result<ArrayFormatter, ()>> = (0..batch.num_columns())
            .map(|col_idx| {
                let col = batch.column(col_idx);
                ArrayFormatter::try_new(col.as_ref(), &Default::default()).map_err(|_| ())
            })
            .collect();
        for row_idx in 0..batch.num_rows() {
            let mut row = Vec::with_capacity(formatters.len());
            for formatter in &formatters {
                let val = match formatter {
                    Ok(f) => f.value(row_idx).to_string(),
                    Err(()) => "<error>".to_string(),
                };
                row.push(val);
            }
            rows.push(row);
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::array::{ArrayRef, Int64Array, StringArray};
    use arrow::datatypes::{DataType, Field, Schema};
    use std::sync::Arc;

    /// A batch with two columns of different types and a null in each, so a
    /// bug that mixes up which formatter belongs to which column (e.g.
    /// reusing column 0's formatter for every column, which is exactly the
    /// mistake the per-column hoist invites) produces visibly wrong output
    /// rather than coincidentally-correct output.
    fn sample_batch() -> RecordBatch {
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, true),
            Field::new("name", DataType::Utf8, true),
        ]));
        let ids: ArrayRef = Arc::new(Int64Array::from(vec![Some(1), None, Some(3)]));
        let names: ArrayRef = Arc::new(StringArray::from(vec![Some("alice"), Some("bob"), None]));
        RecordBatch::try_new(schema, vec![ids, names]).unwrap()
    }

    #[test]
    fn matches_the_naive_per_cell_implementation() {
        // The pre-hoist implementation, kept here only as an oracle: build a
        // fresh `ArrayFormatter` inside the row loop instead of once per
        // column. If the hoisted version ever attaches the wrong formatter
        // to the wrong column, or reuses one formatter across every column,
        // this comparison catches it even though both implementations
        // *look* like they'd agree.
        fn naive(batches: &[RecordBatch]) -> Vec<Vec<String>> {
            let mut rows = Vec::new();
            for batch in batches {
                for row_idx in 0..batch.num_rows() {
                    let mut row = Vec::new();
                    for col_idx in 0..batch.num_columns() {
                        let col = batch.column(col_idx);
                        let formatter = ArrayFormatter::try_new(col.as_ref(), &Default::default());
                        let val = match formatter {
                            Ok(f) => f.value(row_idx).to_string(),
                            Err(_) => "<error>".to_string(),
                        };
                        row.push(val);
                    }
                    rows.push(row);
                }
            }
            rows
        }

        let batches = vec![sample_batch()];
        assert_eq!(format_batches_to_strings(&batches), naive(&batches));
    }

    #[test]
    fn renders_expected_values_including_nulls() {
        let rows = format_batches_to_strings(&[sample_batch()]);
        assert_eq!(
            rows,
            vec![
                vec!["1".to_string(), "alice".to_string()],
                vec!["".to_string(), "bob".to_string()],
                vec!["3".to_string(), "".to_string()],
            ]
        );
    }

    #[test]
    fn multiple_batches_are_each_formatted_with_their_own_formatters() {
        // A second batch with different column types than the first would
        // panic or misformat if formatters built for batch 1 leaked into
        // batch 2's row loop.
        let schema2 = Arc::new(Schema::new(vec![Field::new(
            "flag",
            DataType::Boolean,
            false,
        )]));
        let flags: ArrayRef = Arc::new(arrow::array::BooleanArray::from(vec![true, false]));
        let batch2 = RecordBatch::try_new(schema2, vec![flags]).unwrap();

        let rows = format_batches_to_strings(&[sample_batch(), batch2]);
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[3], vec!["true".to_string()]);
        assert_eq!(rows[4], vec!["false".to_string()]);
    }

    /// Not a correctness gate — timing varies by machine and this is
    /// `#[ignore]`d so `cargo test --workspace` never runs it. Run directly
    /// with:
    ///   cargo test --release -p pq-tui format_perf -- --ignored --nocapture
    /// to compare the per-cell and per-column-per-batch formatter
    /// construction strategies on the same data.
    #[test]
    #[ignore]
    fn format_perf_per_cell_vs_per_column() {
        fn naive(batches: &[RecordBatch]) -> Vec<Vec<String>> {
            let mut rows = Vec::new();
            for batch in batches {
                for row_idx in 0..batch.num_rows() {
                    let mut row = Vec::new();
                    for col_idx in 0..batch.num_columns() {
                        let col = batch.column(col_idx);
                        let formatter = ArrayFormatter::try_new(col.as_ref(), &Default::default());
                        let val = match formatter {
                            Ok(f) => f.value(row_idx).to_string(),
                            Err(_) => "<error>".to_string(),
                        };
                        row.push(val);
                    }
                    rows.push(row);
                }
            }
            rows
        }

        // One page's worth of rows (PAGE_SIZE) x 10 columns, the shape of a
        // real `view`/`cat` scroll fetch.
        let num_rows = PAGE_SIZE;
        let num_cols = 10;
        let schema = Arc::new(Schema::new(
            (0..num_cols)
                .map(|i| Field::new(format!("c{i}"), DataType::Int64, false))
                .collect::<Vec<_>>(),
        ));
        let cols: Vec<ArrayRef> = (0..num_cols)
            .map(|_| {
                Arc::new(Int64Array::from((0..num_rows as i64).collect::<Vec<_>>())) as ArrayRef
            })
            .collect();
        let batch = RecordBatch::try_new(schema, cols).unwrap();
        let batches = [batch];

        const ITERS: usize = 200;

        let mut naive_times = Vec::with_capacity(ITERS);
        for _ in 0..ITERS {
            let start = std::time::Instant::now();
            std::hint::black_box(naive(&batches));
            naive_times.push(start.elapsed());
        }

        let mut hoisted_times = Vec::with_capacity(ITERS);
        for _ in 0..ITERS {
            let start = std::time::Instant::now();
            std::hint::black_box(format_batches_to_strings(&batches));
            hoisted_times.push(start.elapsed());
        }

        let mean = |times: &[std::time::Duration]| {
            times.iter().sum::<std::time::Duration>() / times.len() as u32
        };
        let min = |times: &[std::time::Duration]| *times.iter().min().unwrap();

        eprintln!(
            "naive (per-cell): mean={:?} min={:?} over {ITERS} iters, {num_rows} rows x {num_cols} cols",
            mean(&naive_times),
            min(&naive_times),
        );
        eprintln!(
            "hoisted (per-column): mean={:?} min={:?} over {ITERS} iters, {num_rows} rows x {num_cols} cols",
            mean(&hoisted_times),
            min(&hoisted_times),
        );
    }
}
