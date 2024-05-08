//! This is a tool for ingesting subsets of cache.nixos.org into its own flattened castore format.
//! Currently, produced chunks are not preserved, and this purely serves as a way of measuring
//! compression/deduplication ratios for various chunking and compression parameters.
//!
//! NARs to be ingested are read from `ingest.parquet`, and filtered by an SQL expression provided as a program argument.
//! The `file_hash` column should contain SHA-256 hashes of the compressed data, corresponding to the `FileHash` narinfo field.
//! The `compression` column should contain either `"bzip2"` or `"xz"`, corresponding to the `Compression` narinfo field.
//! Additional columns are ignored, but can be used by the SQL filter expression.
//!
//! flatstore protobufs are written to a sled database named `crunch.db`, addressed by file hash.

use crunch_v2::proto;

mod remote;

use anyhow::Result;
use clap::Parser;
use futures::{stream, StreamExt, TryStreamExt};
use indicatif::{ProgressBar, ProgressStyle};
use std::{
    io::{self, BufRead, Read, Write},
    path::PathBuf,
    ptr,
};

use polars::{
    prelude::{col, LazyFrame, ScanArgsParquet},
    sql::sql_expr,
};

use fastcdc::v2020::{ChunkData, StreamCDC};
use nix_compat::nar::reader as nar;

use digest::Digest;
use prost::Message;
use sha2::Sha256;

#[derive(Parser)]
struct Args {
    /// Path to an existing parquet file.
    /// The `file_hash` column should contain SHA-256 hashes of the compressed
    /// data, corresponding to the `FileHash` narinfo field.
    /// The `compression` column should contain either `"bzip2"` or `"xz"`,
    /// corresponding to the `Compression` narinfo field.
    /// Additional columns are ignored, but can be used by the SQL filter expression.
    #[clap(long, default_value = "ingest.parquet")]
    infile: PathBuf,

    /// Filter expression to filter elements in the parquet file for.
    filter: String,

    /// Average chunk size for FastCDC, in KiB.
    /// min value is half, max value double of that number.
    #[clap(long, default_value_t = 256)]
    avg_chunk_size: u32,

    /// Path to the sled database where results are written to (flatstore
    /// protobufs, addressed by file hash).
    #[clap(long, default_value = "crunch.db")]
    outfile: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let filter = sql_expr(args.filter)?;
    let avg_chunk_size = args.avg_chunk_size * 1024;

    let df = LazyFrame::scan_parquet(&args.infile, ScanArgsParquet::default())?
        .filter(filter)
        .select([col("file_hash"), col("compression")])
        .drop_nulls(None)
        .collect()?;

    let progress = ProgressBar::new(df.height() as u64).with_style(ProgressStyle::with_template(
        "{elapsed_precise}/{duration_precise} {wide_bar} {pos}/{len}",
    )?);

    let file_hash = df
        .column("file_hash")?
        .binary()?
        .into_iter()
        .map(|h| -> [u8; 32] { h.unwrap().try_into().unwrap() });

    let compression = df
        .column("compression")?
        .utf8()?
        .into_iter()
        .map(|c| c.unwrap());

    let db: sled::Db = sled::open(args.outfile).unwrap();
    let files_tree = db.open_tree("files").unwrap();

    let res = stream::iter(file_hash.zip(compression))
        .map(Ok)
        .try_for_each_concurrent(Some(16), |(file_hash, compression)| {
            let progress = progress.clone();
            let files_tree = files_tree.clone();
            async move {
                if files_tree.contains_key(&file_hash)? {
                    progress.inc(1);
                    return Ok(());
                }

                let reader = remote::nar(file_hash, compression).await?;

                tokio::task::spawn_blocking(move || {
                    let mut reader = Sha256Reader::from(reader);

                    let path =
                        ingest(nar::open(&mut reader)?, vec![], avg_chunk_size).map(|node| {
                            proto::Path {
                                nar_hash: reader.finalize().as_slice().into(),
                                node: Some(node),
                            }
                        })?;

                    files_tree.insert(file_hash, path.encode_to_vec())?;
                    progress.inc(1);

                    Ok::<_, anyhow::Error>(())
                })
                .await?
            }
        })
        .await;

    let flush = files_tree.flush_async().await;

    res?;
    flush?;

    Ok(())
}

fn ingest(node: nar::Node, name: Vec<u8>, avg_chunk_size: u32) -> Result<proto::path::Node> {
    match node {
        nar::Node::Symlink { target } => Ok(proto::path::Node::Symlink(proto::SymlinkNode {
            name,
            target,
        })),

        nar::Node::Directory(mut reader) => {
            let mut directories = vec![];
            let mut files = vec![];
            let mut symlinks = vec![];

            while let Some(node) = reader.next()? {
                match ingest(node.node, node.name.to_owned(), avg_chunk_size)? {
                    proto::path::Node::Directory(node) => {
                        directories.push(node);
                    }
                    proto::path::Node::File(node) => {
                        files.push(node);
                    }
                    proto::path::Node::Symlink(node) => {
                        symlinks.push(node);
                    }
                }
            }

            Ok(proto::path::Node::Directory(proto::DirectoryNode {
                name,
                directories,
                files,
                symlinks,
            }))
        }

        nar::Node::File { executable, reader } => {
            let mut reader = B3Reader::from(reader);
            let mut chunks = vec![];

            for chunk in StreamCDC::new(
                &mut reader,
                avg_chunk_size / 2,
                avg_chunk_size,
                avg_chunk_size * 2,
            ) {
                let ChunkData {
                    length: size, data, ..
                } = chunk?;

                let hash = blake3::hash(&data);
                let size_compressed = zstd_size(&data, 9);

                chunks.push(proto::Chunk {
                    hash: hash.as_bytes().as_slice().into(),
                    size: size.try_into().unwrap(),
                    size_compressed: size_compressed.try_into().unwrap(),
                });
            }

            Ok(proto::path::Node::File(proto::FileNode {
                name,
                hash: reader.finalize().as_bytes().as_slice().into(),
                chunks,
                executable,
            }))
        }
    }
}

struct Sha256Reader<R> {
    inner: R,
    hasher: Sha256,
    buf: *const [u8],
}

const ZERO_BUF: *const [u8] = ptr::slice_from_raw_parts(1 as *const u8, 0);

unsafe impl<R: Send> Send for Sha256Reader<R> {}

impl<R> From<R> for Sha256Reader<R> {
    fn from(value: R) -> Self {
        Self {
            inner: value,
            hasher: Sha256::new(),
            buf: ZERO_BUF,
        }
    }
}

impl<R: Read> Read for Sha256Reader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.buf = ZERO_BUF;
        let n = self.inner.read(buf)?;
        self.hasher.update(&buf[..n]);
        Ok(n)
    }
}

impl<R: BufRead> BufRead for Sha256Reader<R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.buf = ZERO_BUF;
        let buf = self.inner.fill_buf()?;
        self.buf = buf as *const [u8];
        Ok(buf)
    }

    fn consume(&mut self, amt: usize) {
        // UNSAFETY: This assumes that `R::consume` doesn't invalidate the buffer.
        // That's not a sound assumption in general, though it is likely to hold.
        // TODO(edef): refactor this codebase to write a fresh NAR for verification purposes
        // we already buffer full chunks, so there's no pressing need to reuse the input buffers
        unsafe {
            let (head, buf) = (*self.buf).split_at(amt);
            self.buf = buf as *const [u8];
            self.hasher.update(head);
            self.inner.consume(amt);
        }
    }
}

impl<R> Sha256Reader<R> {
    fn finalize(self) -> [u8; 32] {
        self.hasher.finalize().into()
    }
}

struct B3Reader<R> {
    inner: R,
    hasher: blake3::Hasher,
}

impl<R> From<R> for B3Reader<R> {
    fn from(value: R) -> Self {
        Self {
            inner: value,
            hasher: blake3::Hasher::new(),
        }
    }
}

impl<R: Read> Read for B3Reader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.hasher.update(&buf[..n]);
        Ok(n)
    }
}

impl<R> B3Reader<R> {
    fn finalize(self) -> blake3::Hash {
        self.hasher.finalize()
    }
}

fn zstd_size(data: &[u8], level: i32) -> u64 {
    let mut w = zstd::Encoder::new(CountingWriter::default(), level).unwrap();
    w.write_all(&data).unwrap();
    let CountingWriter(size) = w.finish().unwrap();
    size
}

#[derive(Default)]
struct CountingWriter(u64);

impl Write for CountingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0 += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
