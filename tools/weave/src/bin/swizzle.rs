//! Swizzle reads a `narinfo.parquet` file, usually produced by `narinfo2parquet`.
//!
//! It swizzles the reference list, ie it converts the references from absolute,
//! global identifiers (store path hashes) to indices into the `store_path_hash`
//! column (ie, row numbers), so that we can later walk the reference graph
//! efficiently.
//!
//! Path hashes are represented as non-null, 20-byte `Binary` values.
//! The indices are represented as 32-bit unsigned integers, with in-band nulls
//! represented by [INDEX_NULL] (the all-1 bit pattern), to permit swizzling
//! partial datasets.
//!
//! In essence, it converts from names to pointers, so that `weave` can simply
//! chase pointers to trace the live set. This replaces an `O(log(n))` lookup
//! with `O(1)` indexing, and produces a much denser representation that actually
//! fits in memory.
//!
//! The in-memory representation is at least 80% smaller, and the indices compress
//! well in Parquet due to both temporal locality of reference and the power law
//! distribution of reference "popularity".
//!
//! Only two columns are read from `narinfo.parquet`:
//!
//!  * `store_path_hash :: PathHash`
//!  * `references :: List[PathHash]`
//!
//! Output is written to `narinfo-references.parquet` in the form of a single
//! `List[u32]` column, `reference_idxs`.
//!
//! This file is inherently bound to the corresponding `narinfo.parquet`,
//! since it essentially contains pointers into this file.

use anyhow::Result;
use hashbrown::HashTable;
use polars::prelude::*;
use rayon::prelude::*;
use std::fs::File;
use tokio::runtime::Runtime;

use weave::{as_fixed_binary, hash64, load_ph_array, DONE, INDEX_NULL};

fn main() -> Result<()> {
    let ph_array = load_ph_array()?;

    // TODO(edef): re-parallelise this
    // We originally parallelised on chunks, but ph_array is only a single chunk, due to how Parquet loading works.
    // TODO(edef): outline the 64-bit hash prefix? it's an indirection, but it saves ~2G of memory
    eprint!("… build index\r");
    let ph_map: HashTable<(u64, u32)> = {
        let mut ph_map = HashTable::with_capacity(ph_array.len());

        for (offset, item) in ph_array.iter().enumerate() {
            let offset = offset as u32;
            let hash = hash64(item);
            ph_map.insert_unique(hash, (hash, offset), |&(hash, _)| hash);
        }

        ph_map
    };
    eprintln!("{DONE}");

    eprint!("… swizzle references\r");
    let mut pq = ParquetReader::new(File::open("narinfo.parquet")?)
        .with_columns(Some(vec!["references".into()]))
        .batched(1 << 16)?;

    let mut reference_idxs =
        Series::new_empty("reference_idxs", &DataType::List(DataType::UInt32.into()));

    let mut bounce = vec![];
    let runtime = Runtime::new()?;
    while let Some(batches) = runtime.block_on(pq.next_batches(48))? {
        batches
            .into_par_iter()
            .map(|df| -> ListChunked {
                df.column("references")
                    .unwrap()
                    .list()
                    .unwrap()
                    .apply_to_inner(&|series: Series| -> PolarsResult<Series> {
                        let series = series.binary()?;
                        let mut out: Vec<u32> = Vec::with_capacity(series.len());

                        out.extend(as_fixed_binary::<20>(series).flat_map(|xs| xs).map(|key| {
                            let hash = hash64(&key);
                            ph_map
                                .find(hash, |&(candidate_hash, candidate_index)| {
                                    candidate_hash == hash
                                        && &ph_array[candidate_index as usize] == key
                                })
                                .map(|&(_, index)| index)
                                .unwrap_or(INDEX_NULL)
                        }));

                        Ok(Series::from_vec("reference_idxs", out))
                    })
                    .unwrap()
            })
            .collect_into_vec(&mut bounce);

        for batch in bounce.drain(..) {
            reference_idxs.append(&batch.into_series())?;
        }
    }
    eprintln!("{DONE}");

    eprint!("… writing output\r");
    ParquetWriter::new(File::create("narinfo-references.parquet")?).finish(&mut df! {
        "reference_idxs" => reference_idxs,
    }?)?;
    eprintln!("{DONE}");

    Ok(())
}
