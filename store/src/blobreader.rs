use std::io::{self, Cursor, Read, Write};

use data_encoding::BASE64;

use crate::{chunkservice::ChunkService, proto};

/// BlobReader implements reading of a blob, by querying individual chunks.
///
/// It doesn't talk to BlobService, but assumes something has already fetched
/// blob_meta already.
pub struct BlobReader<'a, CS: ChunkService> {
    // used to look up chunks
    chunk_service: &'a CS,

    // internal iterator over chunk hashes and their sizes
    chunks_iter: std::vec::IntoIter<proto::blob_meta::ChunkMeta>,

    // If a chunk was partially read (if buf.len() < chunk.size),
    // a cursor to its contents are stored here.
    current_chunk: Option<Cursor<Vec<u8>>>,
}

impl<'a, CS: ChunkService> BlobReader<'a, CS> {
    pub fn open(chunk_service: &'a CS, blob_meta: proto::BlobMeta) -> Self {
        Self {
            chunk_service,
            chunks_iter: blob_meta.chunks.into_iter(),
            current_chunk: None,
        }
    }

    /// reads (up to n bytes) from the current chunk into buf (if there is
    /// a chunk).
    ///
    /// If it arrives at the end of the chunk, sets it back to None.
    /// Returns a io::Result<usize> of the bytes read from the chunk.
    fn read_from_current_chunk<W: std::io::Write>(
        &mut self,
        m: usize,
        buf: &mut W,
    ) -> std::io::Result<usize> {
        // If there's still something in partial_chunk, read from there
        // (up to m: usize bytes) and return the number of bytes read.
        if let Some(current_chunk) = &mut self.current_chunk {
            let result = io::copy(&mut current_chunk.take(m as u64), buf);

            match result {
                Ok(n) => {
                    // if we were not able to read all off m bytes,
                    // this means we arrived at the end of the chunk.
                    if n < m as u64 {
                        self.current_chunk = None
                    }

                    // n can never be > m, so downcasting this to usize is ok.
                    Ok(n as usize)
                }
                Err(e) => Err(e),
            }
        } else {
            Ok(0)
        }
    }
}

impl<CS: ChunkService> std::io::Read for BlobReader<'_, CS> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let read_max = buf.len();
        let mut bytes_read = 0_usize;
        let mut buf_w = std::io::BufWriter::new(buf);

        // read up to buf.len() bytes into buf, by reading from the current
        // chunk and subsequent ones.
        loop {
            // try to fill buf with bytes from the current chunk
            // (if there's still one)
            let n = self.read_from_current_chunk(read_max - bytes_read, &mut buf_w)?;
            bytes_read += n;

            // We want to make sure we don't accidentially read past more than
            // we're allowed to.
            assert!(bytes_read <= read_max);

            // buf is entirerly filled, we're done.
            if bytes_read == read_max {
                buf_w.flush()?;
                break Ok(bytes_read);
            }

            // Otherwise, bytes_read is < read_max, so we could still write
            // more to buf.
            // Check if we have more chunks to read from.
            match self.chunks_iter.next() {
                // No more chunks, we're done.
                None => {
                    buf_w.flush()?;
                    return Ok(bytes_read);
                }
                // There's another chunk to visit, fetch its contents
                Some(chunk_meta) => match self.chunk_service.get(&chunk_meta.digest) {
                    // Fetch successful, put it into `self.current_chunk` and restart the loop.
                    Ok(Some(chunk_data)) => {
                        // make sure the size matches what chunk_meta says as well.
                        if chunk_data.len() as u32 != chunk_meta.size {
                            break Err(std::io::Error::new(
                                io::ErrorKind::InvalidData,
                                format!(
                                    "chunk_service returned chunk with wrong size for {}, expected {}, got {}",
                                    BASE64.encode(&chunk_meta.digest), chunk_meta.size, chunk_data.len()
                                )
                            ));
                        }
                        self.current_chunk = Some(Cursor::new(chunk_data));
                    }
                    // Chunk requested does not exist
                    Ok(None) => {
                        break Err(std::io::Error::new(
                            io::ErrorKind::NotFound,
                            format!("chunk {} not found", BASE64.encode(&chunk_meta.digest)),
                        ))
                    }
                    // Error occured while fetching the next chunk, propagate the error from the chunk service
                    Err(e) => {
                        break Err(std::io::Error::new(io::ErrorKind::InvalidData, e));
                    }
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BlobReader;
    use crate::chunkservice::ChunkService;
    use crate::proto;
    use crate::tests::fixtures::DUMMY_DATA_1;
    use crate::tests::fixtures::DUMMY_DATA_2;
    use crate::tests::fixtures::DUMMY_DIGEST;
    use crate::tests::utils::gen_chunk_service;
    use std::io::Cursor;
    use std::io::Read;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    /// reading from a blobmeta with zero chunks should produce zero bytes.
    fn empty_blobmeta() -> anyhow::Result<()> {
        let tmpdir = TempDir::new()?;
        let chunk_service = gen_chunk_service(tmpdir.path());

        let blobmeta = proto::BlobMeta {
            chunks: vec![],
            inline_bao: vec![],
        };

        let mut blob_reader = BlobReader::open(&chunk_service, blobmeta);
        let mut buf = Cursor::new(Vec::new());

        let res = std::io::copy(&mut blob_reader, &mut buf);

        assert_eq!(0, res.unwrap());

        Ok(())
    }

    #[test]
    /// trying to read something where the chunk doesn't exist should fail
    fn missing_chunk_fail() -> anyhow::Result<()> {
        let tmpdir = TempDir::new()?;
        let chunk_service = gen_chunk_service(tmpdir.path());

        let blobmeta = proto::BlobMeta {
            chunks: vec![proto::blob_meta::ChunkMeta {
                digest: DUMMY_DIGEST.to_vec(),
                size: 42,
            }],
            inline_bao: vec![],
        };

        let mut blob_reader = BlobReader::open(&chunk_service, blobmeta);
        let mut buf = Cursor::new(Vec::new());

        let res = std::io::copy(&mut blob_reader, &mut buf);

        assert!(res.is_err());

        Ok(())
    }

    #[test]
    /// read something containing the single (empty) chunk
    fn empty_chunk() -> anyhow::Result<()> {
        let tmpdir = TempDir::new()?;
        let chunk_service = gen_chunk_service(tmpdir.path());

        // insert a single chunk
        let dgst = chunk_service.put(vec![]).expect("must succeed");

        // assemble a blobmeta
        let blobmeta = proto::BlobMeta {
            chunks: vec![proto::blob_meta::ChunkMeta {
                digest: dgst,
                size: 0,
            }],
            inline_bao: vec![],
        };

        let mut blob_reader = BlobReader::open(&chunk_service, blobmeta);

        let mut buf: Vec<u8> = Vec::new();

        let res =
            std::io::copy(&mut blob_reader, &mut Cursor::new(&mut buf)).expect("must succeed");

        assert_eq!(res, 0, "number of bytes read must match");
        assert!(buf.is_empty(), "buf must be empty");

        Ok(())
    }

    /// read something which contains a single chunk
    #[test]
    fn single_chunk() -> anyhow::Result<()> {
        let tmpdir = TempDir::new()?;
        let chunk_service = gen_chunk_service(tmpdir.path());

        // insert a single chunk
        let dgst = chunk_service
            .put(DUMMY_DATA_1.clone())
            .expect("must succeed");

        // assemble a blobmeta
        let blobmeta = proto::BlobMeta {
            chunks: vec![proto::blob_meta::ChunkMeta {
                digest: dgst,
                size: 3,
            }],
            inline_bao: vec![],
        };

        let mut blob_reader = BlobReader::open(&chunk_service, blobmeta);

        let mut buf: Vec<u8> = Vec::new();

        let res =
            std::io::copy(&mut blob_reader, &mut Cursor::new(&mut buf)).expect("must succeed");

        assert_eq!(res, 3, "number of bytes read must match");
        assert_eq!(DUMMY_DATA_1[..], buf[..], "data read must match");

        Ok(())
    }

    /// read something referring to a chunk, but with wrong size
    #[test]
    fn wrong_size_fail() -> anyhow::Result<()> {
        let tmpdir = TempDir::new()?;
        let chunk_service = gen_chunk_service(tmpdir.path());

        // insert chunks
        let dgst_1 = chunk_service
            .put(DUMMY_DATA_1.clone())
            .expect("must succeed");

        // assemble a blobmeta
        let blobmeta = proto::BlobMeta {
            chunks: vec![proto::blob_meta::ChunkMeta {
                digest: dgst_1,
                size: 42,
            }],
            inline_bao: vec![],
        };

        let mut blob_reader = BlobReader::open(&chunk_service, blobmeta);

        let mut buf: Vec<u8> = Vec::new();

        let res = std::io::copy(&mut blob_reader, &mut Cursor::new(&mut buf));

        assert!(res.is_err(), "reading must fail");

        Ok(())
    }

    /// read something referring to multiple chunks
    #[test]
    fn multiple_chunks() -> anyhow::Result<()> {
        let tmpdir = TempDir::new()?;
        let chunk_service = gen_chunk_service(tmpdir.path());

        // insert chunks
        let dgst_1 = chunk_service
            .put(DUMMY_DATA_1.clone())
            .expect("must succeed");
        let dgst_2 = chunk_service
            .put(DUMMY_DATA_2.clone())
            .expect("must succeed");

        // assemble a blobmeta
        let blobmeta = proto::BlobMeta {
            chunks: vec![
                proto::blob_meta::ChunkMeta {
                    digest: dgst_1.clone(),
                    size: 3,
                },
                proto::blob_meta::ChunkMeta {
                    digest: dgst_2,
                    size: 2,
                },
                proto::blob_meta::ChunkMeta {
                    digest: dgst_1,
                    size: 3,
                },
            ],
            inline_bao: vec![],
        };

        // assemble ecpected data
        let mut expected_data: Vec<u8> = Vec::new();
        expected_data.extend_from_slice(&DUMMY_DATA_1[..]);
        expected_data.extend_from_slice(&DUMMY_DATA_2[..]);
        expected_data.extend_from_slice(&DUMMY_DATA_1[..]);

        // read via io::copy
        {
            let mut blob_reader = BlobReader::open(&chunk_service, blobmeta.clone());

            let mut buf: Vec<u8> = Vec::new();

            let res =
                std::io::copy(&mut blob_reader, &mut Cursor::new(&mut buf)).expect("must succeed");

            assert_eq!(8, res, "number of bytes read must match");

            assert_eq!(expected_data[..], buf[..], "data read must match");
        }

        // now read the same thing again, but not via io::copy, but individually
        {
            let mut blob_reader = BlobReader::open(&chunk_service, blobmeta);

            let mut buf: Vec<u8> = Vec::new();
            let mut cursor = Cursor::new(&mut buf);

            let mut bytes_read = 0;

            loop {
                let mut smallbuf = [0xff; 1];
                match blob_reader.read(&mut smallbuf) {
                    Ok(n) => {
                        if n == 0 {
                            break;
                        }
                        let w_b = cursor.write(&smallbuf).unwrap();
                        assert_eq!(n, w_b);
                        bytes_read += w_b;
                    }
                    Err(_) => {
                        panic!("error occured during read");
                    }
                }
            }

            assert_eq!(8, bytes_read, "number of bytes read must match");
            assert_eq!(expected_data[..], buf[..], "data read must match");
        }

        Ok(())
    }
}
