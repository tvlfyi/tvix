//! narinfo2parquet operates on a narinfo.zst directory produced by turbofetch.
//! It takes the name of a segment file in `narinfo.zst` and writes a Parquet file
//! with the same name into the `narinfo.pq` directory.
//!
//! Run it under GNU Parallel for parallelism:
//! ```shell
//! mkdir narinfo.pq && ls narinfo.zst | parallel --bar 'narinfo2parquet {}'
//! ```

use anyhow::{bail, Context, Result};
use jemallocator::Jemalloc;
use nix_compat::{
    narinfo::{self, NarInfo},
    nixbase32,
};
use polars::{io::parquet::ParquetWriter, prelude::*};
use std::{
    fs::{self, File},
    io::{self, BufRead, BufReader, Read},
    path::Path,
};
use tempfile_fast::PersistableTempFile;

#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

fn main() -> Result<()> {
    let file_name = std::env::args().nth(1).expect("file name missing");
    let input_path = Path::new("narinfo.zst").join(&file_name);
    let output_path = Path::new("narinfo.pq").join(&file_name);

    match fs::metadata(&output_path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => bail!(e),
        Ok(_) => bail!("output path already exists: {output_path:?}"),
    }

    let reader = File::open(input_path).and_then(zstd::Decoder::new)?;
    let mut frame = FrameBuilder::default();

    for_each(reader, |s| {
        let entry = NarInfo::parse(&s).context("couldn't parse entry:\n{s}")?;
        frame.push(&entry);
        Ok(())
    })?;

    let mut frame = frame.finish();
    let mut writer = PersistableTempFile::new_in(output_path.parent().unwrap())?;

    ParquetWriter::new(&mut writer)
        .with_compression(ParquetCompression::Gzip(None))
        .with_statistics(true)
        .finish(frame.align_chunks())?;

    writer
        .persist_noclobber(output_path)
        .map_err(|e| e.error)
        .context("couldn't commit output file")?;

    Ok(())
}

fn for_each(reader: impl Read, mut f: impl FnMut(&str) -> Result<()>) -> Result<()> {
    let mut reader = BufReader::new(reader);
    let mut group = String::new();
    loop {
        let prev_len = group.len();

        if prev_len > 1024 * 1024 {
            bail!("excessively large segment");
        }

        reader.read_line(&mut group)?;
        let (prev, line) = group.split_at(prev_len);

        // EOF
        if line.is_empty() {
            break;
        }

        // skip empty line
        if line == "\n" {
            group.pop().unwrap();
            continue;
        }

        if !prev.is_empty() && line.starts_with("StorePath:") {
            f(prev)?;
            group.drain(..prev_len);
        }
    }

    if !group.is_empty() {
        f(&group)?;
    }

    Ok(())
}

/// [FrameBuilder] builds a [DataFrame] out of [NarInfo]s.
/// The exact format is still in flux.
///
/// # Example
///
/// ```no_run
/// |narinfos: &[NarInfo]| -> DataFrame {
///     let frame_builder = FrameBuilder::default();
///     narinfos.for_each(|n| frame_builder.push(n));
///     frame_builder.finish()
/// }
/// ```
struct FrameBuilder {
    store_path_hash_str: StringChunkedBuilder,
    store_path_hash: BinaryChunkedBuilder,
    store_path_name: StringChunkedBuilder,
    deriver_hash_str: StringChunkedBuilder,
    deriver_hash: BinaryChunkedBuilder,
    deriver_name: StringChunkedBuilder,
    nar_hash: BinaryChunkedBuilder,
    nar_size: PrimitiveChunkedBuilder<UInt64Type>,
    references: ListBinaryChunkedBuilder,
    ca_algo: CategoricalChunkedBuilder<'static>,
    ca_hash: BinaryChunkedBuilder,
    signature: BinaryChunkedBuilder,
    file_hash: BinaryChunkedBuilder,
    file_size: PrimitiveChunkedBuilder<UInt64Type>,
    compression: CategoricalChunkedBuilder<'static>,
    quirk_references_out_of_order: BooleanChunkedBuilder,
    quirk_nar_hash_hex: BooleanChunkedBuilder,
}

impl Default for FrameBuilder {
    fn default() -> Self {
        Self {
            store_path_hash_str: StringChunkedBuilder::new("store_path_hash_str", 0, 0),
            store_path_hash: BinaryChunkedBuilder::new("store_path_hash", 0, 0),
            store_path_name: StringChunkedBuilder::new("store_path_name", 0, 0),
            deriver_hash_str: StringChunkedBuilder::new("deriver_hash_str", 0, 0),
            deriver_hash: BinaryChunkedBuilder::new("deriver_hash", 0, 0),
            deriver_name: StringChunkedBuilder::new("deriver_name", 0, 0),
            nar_hash: BinaryChunkedBuilder::new("nar_hash", 0, 0),
            nar_size: PrimitiveChunkedBuilder::new("nar_size", 0),
            references: ListBinaryChunkedBuilder::new("references", 0, 0),
            signature: BinaryChunkedBuilder::new("signature", 0, 0),
            ca_algo: CategoricalChunkedBuilder::new("ca_algo", 0, CategoricalOrdering::Lexical),
            ca_hash: BinaryChunkedBuilder::new("ca_hash", 0, 0),
            file_hash: BinaryChunkedBuilder::new("file_hash", 0, 0),
            file_size: PrimitiveChunkedBuilder::new("file_size", 0),
            compression: CategoricalChunkedBuilder::new(
                "compression",
                0,
                CategoricalOrdering::Lexical,
            ),
            quirk_references_out_of_order: BooleanChunkedBuilder::new(
                "quirk_references_out_of_order",
                0,
            ),
            quirk_nar_hash_hex: BooleanChunkedBuilder::new("quirk_nar_hash_hex", 0),
        }
    }
}

impl FrameBuilder {
    fn push(&mut self, entry: &NarInfo) {
        self.store_path_hash_str
            .append_value(nixbase32::encode(entry.store_path.digest()));
        self.store_path_hash.append_value(entry.store_path.digest());
        self.store_path_name.append_value(entry.store_path.name());

        if let Some(deriver) = &entry.deriver {
            self.deriver_hash_str
                .append_value(nixbase32::encode(deriver.digest()));
            self.deriver_hash.append_value(deriver.digest());
            self.deriver_name.append_value(deriver.name());
        } else {
            self.deriver_hash_str.append_null();
            self.deriver_hash.append_null();
            self.deriver_name.append_null();
        }

        self.nar_hash.append_value(&entry.nar_hash);
        self.nar_size.append_value(entry.nar_size);

        self.references
            .append_values_iter(entry.references.iter().map(|r| r.digest().as_slice()));

        assert!(entry.signatures.len() <= 1);
        self.signature
            .append_option(entry.signatures.get(0).map(|sig| {
                assert_eq!(sig.name(), &"cache.nixos.org-1");
                sig.bytes()
            }));

        if let Some(ca) = &entry.ca {
            self.ca_algo.append_value(ca.algo_str());
            self.ca_hash.append_value(ca.hash().digest_as_bytes());
        } else {
            self.ca_algo.append_null();
            self.ca_hash.append_null();
        }

        let file_hash = entry.file_hash.as_ref().unwrap();
        let file_size = entry.file_size.unwrap();

        self.file_hash.append_value(file_hash);
        self.file_size.append_value(file_size);

        let (compression, extension) = match entry.compression {
            Some("bzip2") => ("bzip2", "bz2"),
            Some("xz") => ("xz", "xz"),
            Some("zstd") => ("zstd", "zst"),
            x => panic!("unknown compression algorithm: {x:?}"),
        };

        self.compression.append_value(compression);

        let mut file_name = nixbase32::encode(file_hash);
        file_name.push_str(".nar.");
        file_name.push_str(extension);

        assert_eq!(entry.url.strip_prefix("nar/").unwrap(), file_name);

        {
            use narinfo::Flags;

            self.quirk_references_out_of_order
                .append_value(entry.flags.contains(Flags::REFERENCES_OUT_OF_ORDER));

            self.quirk_nar_hash_hex
                .append_value(entry.flags.contains(Flags::NAR_HASH_HEX));

            let quirks = Flags::REFERENCES_OUT_OF_ORDER | Flags::NAR_HASH_HEX;
            let unknown_flags = entry.flags.difference(quirks);

            assert!(
                unknown_flags.is_empty(),
                "rejecting flags: {unknown_flags:?}"
            );
        }
    }

    fn finish(mut self) -> DataFrame {
        df! {
            "store_path_hash_str" => self.store_path_hash_str.finish().into_series(),
            "store_path_hash" => self.store_path_hash.finish().into_series(),
            "store_path_name" => self.store_path_name.finish().into_series(),
            "deriver_hash_str" => self.deriver_hash_str.finish().into_series(),
            "deriver_hash" => self.deriver_hash.finish().into_series(),
            "deriver_name" => self.deriver_name.finish().into_series(),
            "nar_hash" => self.nar_hash.finish().into_series(),
            "nar_size" => self.nar_size.finish().into_series(),
            "references" => self.references.finish().into_series(),
            "signature" => self.signature.finish().into_series(),
            "ca_algo" => self.ca_algo.finish().into_series(),
            "ca_hash" => self.ca_hash.finish().into_series(),
            "file_hash" => self.file_hash.finish().into_series(),
            "file_size" => self.file_size.finish().into_series(),
            "compression" => self.compression.finish().into_series(),
            "quirk_references_out_of_order" => self.quirk_references_out_of_order.finish().into_series(),
            "quirk_nar_hash_hex" => self.quirk_nar_hash_hex.finish().into_series()
        }
        .unwrap()
    }
}
