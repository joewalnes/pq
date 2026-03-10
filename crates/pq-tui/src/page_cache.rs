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
pub fn format_batches_to_strings(batches: &[RecordBatch]) -> Vec<Vec<String>> {
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
