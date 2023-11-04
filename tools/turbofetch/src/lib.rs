use std::{mem::MaybeUninit, str};
use tokio::io::{self, AsyncRead, AsyncReadExt};

pub use buffer::Buffer;
mod buffer;

/// Read as much data into `buffer` as possible.
/// Returns [io::ErrorKind::OutOfMemory] if the buffer is already full.
async fn slurp(buffer: &mut Buffer, sock: &mut (impl AsyncRead + Unpin)) -> io::Result<()> {
    match buffer.space() {
        [] => Err(io::Error::new(io::ErrorKind::OutOfMemory, "buffer filled")),
        buf => {
            let n = sock.read(buf).await?;
            if n == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            buffer.commit(n);

            Ok(())
        }
    }
}

fn get_content_length(headers: &[httparse::Header]) -> io::Result<u64> {
    for header in headers {
        if header.name == "Transfer-Encoding" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Transfer-Encoding is unsupported",
            ));
        }

        if header.name == "Content-Length" {
            return str::from_utf8(header.value)
                .ok()
                .and_then(|v| v.parse().ok())
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "invalid Content-Length")
                });
        }
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "Content-Length missing",
    ))
}

/// Read an HTTP response from `sock` using `buffer`, returning the response body.
/// Returns an error if anything but 200 OK is received.
///
/// The buffer must have enough space to contain the entire response body.
/// If there is not enough space, [io::ErrorKind::OutOfMemory] is returned.
///
/// The HTTP response must use `Content-Length`, without `Transfer-Encoding`.
pub async fn parse_response<'a>(
    sock: &mut (impl AsyncRead + Unpin),
    buffer: &'a mut Buffer,
) -> io::Result<&'a [u8]> {
    let body_len = loop {
        let mut headers = [MaybeUninit::uninit(); 16];
        let mut response = httparse::Response::new(&mut []);
        let status = httparse::ParserConfig::default()
            .parse_response_with_uninit_headers(&mut response, buffer.data(), &mut headers)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        if let httparse::Status::Complete(n) = status {
            buffer.consume(n);

            let code = response.code.unwrap();
            if code != 200 {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!("HTTP response {code}"),
                ));
            }

            break get_content_length(response.headers)?;
        }

        slurp(buffer, sock).await?;
    };

    let buf_len = buffer.space().len() + buffer.data().len();

    if body_len > buf_len as u64 {
        return Err(io::Error::new(
            io::ErrorKind::OutOfMemory,
            "HTTP response body does not fit in buffer",
        ));
    }

    let body_len = body_len as usize;

    while buffer.data().len() < body_len {
        slurp(buffer, sock).await?;
    }

    let data = buffer.data();
    buffer.consume(body_len);

    Ok(&data[..body_len])
}
