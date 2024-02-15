//! Weave resolves a list of roots from `nixpkgs.roots` against `narinfo.parquet`,
//! and then uses the reference graph from the accompanying `narinfo-references.parquet`
//! produced by `swizzle` to collect the closure of the roots.
//!
//! They are written to `live_idxs.parquet`, which only has one column, representing
//! the row numbers in `narinfo.parquet` corresponding to live paths.

use anyhow::Result;
use hashbrown::{hash_table, HashTable};
use nix_compat::nixbase32;
use rayon::prelude::*;
use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, File},
    ops::Index,
    sync::atomic::{AtomicU32, Ordering},
};

use polars::{
    datatypes::StaticArray,
    export::arrow::{array::UInt32Array, offset::OffsetsBuffer},
    prelude::*,
};

use weave::{hash64, DONE, INDEX_NULL};

fn main() -> Result<()> {
    eprint!("… parse roots\r");
    let roots: PathSet32 = {
        let mut roots = Vec::new();
        fs::read("nixpkgs.roots")?
            .par_chunks_exact(32 + 1)
            .map(|e| nixbase32::decode_fixed::<20>(&e[0..32]).unwrap())
            .collect_into_vec(&mut roots);

        roots.iter().collect()
    };
    eprintln!("{DONE}");

    {
        let ph_array = weave::load_ph_array()?;

        eprint!("… resolve roots\r");
        ph_array.par_iter().enumerate().for_each(|(idx, h)| {
            if let Some(idx_slot) = roots.find(h) {
                idx_slot
                    .compare_exchange(INDEX_NULL, idx as u32, Ordering::SeqCst, Ordering::SeqCst)
                    .expect("duplicate entry");
            }
        });
        eprintln!("{DONE}");
    }

    let mut todo = HashSet::with_capacity(roots.len());
    {
        let mut unknown_roots = 0usize;
        for (_, idx) in roots.table {
            let idx = idx.into_inner();
            if idx == INDEX_NULL {
                unknown_roots += 1;
                continue;
            }
            todo.insert(idx);
        }
        println!("skipping {unknown_roots} unknown roots");
    }

    eprint!("… load reference_idxs\r");
    let ri_array = ParquetReader::new(File::open("narinfo-references.parquet")?)
        .finish()?
        .column("reference_idxs")?
        .list()?
        .clone();

    let ri_array = {
        ChunkedList::new(ri_array.downcast_iter().map(|chunk| {
            (
                chunk.offsets(),
                chunk
                    .values()
                    .as_any()
                    .downcast_ref::<UInt32Array>()
                    .unwrap()
                    .as_slice()
                    .unwrap(),
            )
        }))
    };
    eprintln!("{DONE}");

    let mut seen = todo.clone();
    while !todo.is_empty() {
        println!("todo: {} seen: {}", todo.len(), seen.len());

        todo = todo
            .par_iter()
            .flat_map(|&parent| {
                if parent == INDEX_NULL {
                    return vec![];
                }

                ri_array[parent as usize]
                    .iter()
                    .cloned()
                    .filter(|child| !seen.contains(child))
                    .collect::<Vec<u32>>()
            })
            .collect();

        for &index in &todo {
            seen.insert(index);
        }
    }

    println!("done: {} paths", seen.len());

    if seen.remove(&INDEX_NULL) {
        println!("WARNING: missing edges");
    }

    eprint!("… gathering live set\r");
    let mut seen: Vec<u32> = seen.into_iter().collect();
    seen.par_sort();
    eprintln!("{DONE}");

    eprint!("… writing output\r");
    ParquetWriter::new(File::create("live_idxs.parquet")?).finish(&mut df! {
        "live_idx" => seen,
    }?)?;
    eprintln!("{DONE}");

    Ok(())
}

struct PathSet32 {
    table: HashTable<([u8; 20], AtomicU32)>,
}

impl PathSet32 {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            table: HashTable::with_capacity(capacity),
        }
    }

    fn insert(&mut self, value: &[u8; 20]) -> bool {
        let hash = hash64(value);

        match self
            .table
            .entry(hash, |(x, _)| x == value, |(x, _)| hash64(x))
        {
            hash_table::Entry::Occupied(_) => false,
            hash_table::Entry::Vacant(entry) => {
                entry.insert((*value, AtomicU32::new(INDEX_NULL)));
                true
            }
        }
    }

    fn find(&self, value: &[u8; 20]) -> Option<&AtomicU32> {
        let hash = hash64(value);
        self.table
            .find(hash, |(x, _)| x == value)
            .as_ref()
            .map(|(_, x)| x)
    }

    fn len(&self) -> usize {
        self.table.len()
    }
}

impl<'a> FromIterator<&'a [u8; 20]> for PathSet32 {
    fn from_iter<T: IntoIterator<Item = &'a [u8; 20]>>(iter: T) -> Self {
        let iter = iter.into_iter();
        let mut this = Self::with_capacity(iter.size_hint().0);

        for item in iter {
            this.insert(item);
        }

        this
    }
}

struct ChunkedList<'a, T> {
    by_offset: BTreeMap<usize, (&'a OffsetsBuffer<i64>, &'a [T])>,
}

impl<'a, T> ChunkedList<'a, T> {
    fn new(chunks: impl IntoIterator<Item = (&'a OffsetsBuffer<i64>, &'a [T])>) -> Self {
        let mut next_offset = 0usize;
        ChunkedList {
            by_offset: chunks
                .into_iter()
                .map(|(offsets, values)| {
                    let offset = next_offset;
                    next_offset = next_offset.checked_add(offsets.len_proxy()).unwrap();

                    (offset, (offsets, values))
                })
                .collect(),
        }
    }
}

impl<'a, T> Index<usize> for ChunkedList<'a, T> {
    type Output = [T];

    fn index(&self, index: usize) -> &Self::Output {
        let (&base, &(offsets, values)) = self.by_offset.range(..=index).next_back().unwrap();
        let (start, end) = offsets.start_end(index - base);
        &values[start..end]
    }
}
