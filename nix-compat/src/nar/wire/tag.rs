/// A type implementing Tag represents a static hash set of byte strings,
/// with a very simple perfect hash function: every element has a unique
/// discriminant at a common byte offset. The values of the type represent
/// the members by this single discriminant byte; they are indices into the
/// hash set.
pub trait Tag: Sized {
    /// Discriminant offset
    const OFF: usize;
    /// Minimum variant length
    const MIN: usize;

    /// Minimal suitably sized buffer for reading the wire representation
    /// HACK: This is a workaround for const generics limitations.
    type Buf: AsMut<[u8]> + Send;

    /// Make an instance of [Self::Buf]
    fn make_buf() -> Self::Buf;

    /// Convert a discriminant into the corresponding variant
    fn from_u8(x: u8) -> Option<Self>;

    /// Convert a variant back into the wire representation
    fn as_bytes(&self) -> &'static [u8];
}

/// Generate an enum implementing [Tag], enforcing at compile time that
/// the discriminant values are distinct.
macro_rules! make {
    (
        $(
            $(#[doc = $doc:expr])*
            $vis:vis enum $Enum:ident[$off:expr] {
                $(
                    $(#[doc = $var_doc:expr])*
                    $Var:ident = $TOK:ident,
                )+
            }
        )*
    ) => {
        $(
            $(#[doc = $doc])*
            #[derive(Debug, PartialEq, Eq)]
            #[repr(u8)]
            $vis enum $Enum {
                $(
                    $(#[doc = $var_doc])*
                    $Var = $TOK[$Enum::OFF]
                ),+
            }

            impl Tag for $Enum {
                /// Discriminant offset
                const OFF: usize = $off;
                /// Minimum variant length
                const MIN: usize = tag::min_of(&[$($TOK.len()),+]);

                /// Minimal suitably sized buffer for reading the wire representation
                type Buf = [u8; tag::buf_of(&[$($TOK.len()),+])];

                /// Make an instance of [Self::Buf]
                #[inline(always)]
                fn make_buf() -> Self::Buf {
                    [0u8; tag::buf_of(&[$($TOK.len()),+])]
                }

                /// Convert a discriminant into the corresponding variant
                #[inline(always)]
                fn from_u8(x: u8) -> Option<Self> {
                    #[allow(non_upper_case_globals)]
                    mod __variant {
                        $(
                            pub const $Var: u8 = super::$Enum::$Var as u8;
                        )+
                    }

                    match x {
                        $(__variant::$Var => Some(Self::$Var),)+
                        _ => None
                    }
                }

                /// Convert a variant back into the wire representation
                #[inline(always)]
                fn as_bytes(&self) -> &'static [u8] {
                    match self {
                        $(Self::$Var => &$TOK,)+
                    }
                }
            }
        )*
    };
}

pub(crate) use make;

#[cfg(test)]
mod test {
    use super::super::tag::{self, Tag};

    const TOK_A: [u8; 3] = [0xed, 0xef, 0x1c];
    const TOK_B: [u8; 3] = [0xed, 0xf0, 0x1c];

    const OFFSET: usize = 1;

    make! {
        enum Token[OFFSET] {
            A = TOK_A,
            B = TOK_B,
        }
    }

    #[test]
    fn example() {
        assert_eq!(Token::from_u8(0xed), None);

        let tag = Token::from_u8(0xef).unwrap();
        assert_eq!(tag.as_bytes(), &TOK_A[..]);

        let tag = Token::from_u8(0xf0).unwrap();
        assert_eq!(tag.as_bytes(), &TOK_B[..]);
    }
}

// The following functions are written somewhat unusually,
// since they're const functions that cannot use iterators.

/// Maximum element of a slice
const fn max_of(mut xs: &[usize]) -> usize {
    let mut y = usize::MIN;
    while let &[x, ref tail @ ..] = xs {
        y = if x > y { x } else { y };
        xs = tail;
    }
    y
}

/// Minimum element of a slice
pub const fn min_of(mut xs: &[usize]) -> usize {
    let mut y = usize::MAX;
    while let &[x, ref tail @ ..] = xs {
        y = if x < y { x } else { y };
        xs = tail;
    }
    y
}

/// Minimum buffer size to contain either of `0..Tag::MIN` and `Tag::MIN..`
/// at a particular time, for all possible tag wire representations, given
/// the sizes of all wire representations.
///
/// # Example
///
/// ```plain
/// OFF = 16
/// MIN = 24
/// MAX = 64
///
/// BUF = max(MIN, MAX-MIN)
///     = max(24, 64-24)
///     = max(24, 40)
///     = 40
/// ```
pub const fn buf_of(xs: &[usize]) -> usize {
    max_of(&[min_of(xs), max_of(xs) - min_of(xs)])
}
