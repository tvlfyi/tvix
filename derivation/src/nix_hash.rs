/// CompressHash takes an arbitrary long sequence of bytes (usually a hash
/// digest), and returns a sequence of bytes of length output_size.
/// It's calculated by rotating through the bytes in the output buffer (zero-
/// initialized), and XOR'ing with each byte of the passed input.
/// It consumes 1 byte at a time, and XOR's it with the current value in the
/// output buffer.
pub fn compress_hash(input: &[u8], output_size: usize) -> Vec<u8> {
    let mut output: Vec<u8> = vec![0; output_size];

    for (ii, ch) in input.iter().enumerate() {
        output[ii % output_size] ^= ch;
    }

    output
}
