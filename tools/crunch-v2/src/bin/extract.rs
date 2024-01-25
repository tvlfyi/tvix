//! This tool lossily converts a Sled database produced by crunch-v2 into a Parquet file for analysis.
//! The resulting `crunch.parquet` has columns file_hash`, `nar_hash`, and `chunk`.
//! The first two are SHA-256 hashes of the compressed file and the NAR it decompresses to.
//! `chunk` is a struct array corresponding to [crunch_v2::proto::Chunk] messages.
//! They are concatenated without any additional structure, so nothing but the chunk list is preserved.

use anyhow::Result;
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs::File;
use std::path::PathBuf;

use crunch_v2::proto::{self, path::Node};
use prost::Message;

use polars::{
    chunked_array::builder::AnonymousOwnedListBuilder,
    prelude::{
        df, BinaryChunkedBuilder, ChunkedBuilder, DataFrame, DataType, Field, ListBuilderTrait,
        NamedFrom, ParquetWriter, PrimitiveChunkedBuilder, Series, UInt32Type,
    },
    series::IntoSeries,
};

#[derive(Parser)]
struct Args {
    /// Path to the sled database that's read from.
    #[clap(default_value = "crunch.db")]
    infile: PathBuf,

    /// Path to the resulting parquet file that's written.
    #[clap(default_value = "crunch.parquet")]
    outfile: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let w = ParquetWriter::new(File::create(args.outfile)?);

    let db: sled::Db = sled::open(&args.infile).unwrap();
    let files_tree: sled::Tree = db.open_tree("files").unwrap();

    let progress =
        ProgressBar::new(files_tree.len() as u64).with_style(ProgressStyle::with_template(
            "{elapsed_precise}/{duration_precise} {wide_bar} {pos}/{len}",
        )?);

    let mut frame = FrameBuilder::new();
    for entry in &files_tree {
        let (file_hash, pb) = entry?;
        frame.push(
            file_hash[..].try_into().unwrap(),
            proto::Path::decode(&pb[..])?,
        );
        progress.inc(1);
    }

    w.finish(&mut frame.finish())?;

    Ok(())
}

struct FrameBuilder {
    file_hash: BinaryChunkedBuilder,
    nar_hash: BinaryChunkedBuilder,
    chunk: AnonymousOwnedListBuilder,
}

impl FrameBuilder {
    fn new() -> Self {
        Self {
            file_hash: BinaryChunkedBuilder::new("file_hash", 0, 0),
            nar_hash: BinaryChunkedBuilder::new("nar_hash", 0, 0),
            chunk: AnonymousOwnedListBuilder::new(
                "chunk",
                0,
                Some(DataType::Struct(vec![
                    Field::new("hash", DataType::Binary),
                    Field::new("size", DataType::UInt32),
                    Field::new("size_compressed", DataType::UInt32),
                ])),
            ),
        }
    }

    fn push(&mut self, file_hash: [u8; 32], pb: proto::Path) {
        self.file_hash.append_value(&file_hash[..]);
        self.nar_hash.append_value(pb.nar_hash);
        self.chunk
            .append_series(&ChunkFrameBuilder::new(pb.node.unwrap()))
            .unwrap();
    }

    fn finish(mut self) -> DataFrame {
        df! {
            "file_hash" => self.file_hash.finish().into_series(),
            "nar_hash" => self.nar_hash.finish().into_series(),
            "chunk" => self.chunk.finish().into_series()
        }
        .unwrap()
    }
}

struct ChunkFrameBuilder {
    hash: BinaryChunkedBuilder,
    size: PrimitiveChunkedBuilder<UInt32Type>,
    size_compressed: PrimitiveChunkedBuilder<UInt32Type>,
}

impl ChunkFrameBuilder {
    fn new(node: proto::path::Node) -> Series {
        let mut this = Self {
            hash: BinaryChunkedBuilder::new("hash", 0, 0),
            size: PrimitiveChunkedBuilder::new("size", 0),
            size_compressed: PrimitiveChunkedBuilder::new("size_compressed", 0),
        };

        this.push(node);
        this.finish()
    }

    fn push(&mut self, node: Node) {
        match node {
            Node::Directory(node) => {
                for node in node.files {
                    self.push(Node::File(node));
                }

                for node in node.directories {
                    self.push(Node::Directory(node));
                }
            }
            Node::File(node) => {
                for chunk in node.chunks {
                    self.hash.append_value(&chunk.hash);
                    self.size.append_value(chunk.size);
                    self.size_compressed.append_value(chunk.size_compressed);
                }
            }
            Node::Symlink(_) => {}
        }
    }

    fn finish(self) -> Series {
        df! {
            "hash" => self.hash.finish().into_series(),
            "size" => self.size.finish().into_series(),
            "size_compressed" => self.size_compressed.finish().into_series()
        }
        .unwrap()
        .into_struct("chunk")
        .into_series()
    }
}
