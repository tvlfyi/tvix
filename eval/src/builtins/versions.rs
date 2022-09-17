use std::ops::RangeInclusive;

/// Version strings can be broken up into Parts.
/// One Part represents either a string of digits or characters.
/// '.' and '_' represent deviders between parts and are not included in any part.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Debug)]
pub enum VersionPart<'a> {
    Word(&'a str),
    Number(u64),
}

/// Type used to hold information about a VersionPart during creation
enum InternalPart {
    Number { range: RangeInclusive<usize> },
    Word { range: RangeInclusive<usize> },
    Break,
}

/// An iterator which yields the parts of a version string.
///
/// This can then be directly used to compare two versions
pub struct VersionPartsIter<'a> {
    cached_part: InternalPart,
    iter: std::str::CharIndices<'a>,
    version: &'a str,
}

impl<'a> VersionPartsIter<'a> {
    pub fn new(version: &'a str) -> Self {
        Self {
            cached_part: InternalPart::Break,
            iter: version.char_indices(),
            version,
        }
    }
}

impl<'a> Iterator for VersionPartsIter<'a> {
    type Item = VersionPart<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let char = self.iter.next();

        if char.is_none() {
            let cached_part = std::mem::replace(&mut self.cached_part, InternalPart::Break);
            match cached_part {
                InternalPart::Break => return None,
                InternalPart::Number { range } => {
                    return Some(VersionPart::Number(self.version[range].parse().unwrap()))
                }
                InternalPart::Word { range } => {
                    return Some(VersionPart::Word(&self.version[range]))
                }
            }
        }

        let (pos, char) = char.unwrap();
        match char {
            // Divider encountered
            '.' | '_' => {
                let cached_part = std::mem::replace(&mut self.cached_part, InternalPart::Break);
                match cached_part {
                    InternalPart::Number { range } => {
                        Some(VersionPart::Number(self.version[range].parse().unwrap()))
                    }
                    InternalPart::Word { range } => Some(VersionPart::Word(&self.version[range])),
                    InternalPart::Break => self.next(),
                }
            }

            // digit encountered
            _ if char.is_ascii_digit() => {
                let cached_part = std::mem::replace(
                    &mut self.cached_part,
                    InternalPart::Number { range: pos..=pos },
                );
                match cached_part {
                    InternalPart::Number { range } => {
                        self.cached_part = InternalPart::Number {
                            range: *range.start()..=*range.end() + 1,
                        };
                        self.next()
                    }
                    InternalPart::Word { range } => Some(VersionPart::Word(&self.version[range])),
                    InternalPart::Break => self.next(),
                }
            }

            // char encountered
            _ => {
                let mut cached_part = InternalPart::Word { range: pos..=pos };
                std::mem::swap(&mut cached_part, &mut self.cached_part);
                match cached_part {
                    InternalPart::Word { range } => {
                        self.cached_part = InternalPart::Word {
                            range: *range.start()..=*range.end() + char.len_utf8(),
                        };
                        self.next()
                    }
                    InternalPart::Number { range } => {
                        Some(VersionPart::Number(self.version[range].parse().unwrap()))
                    }
                    InternalPart::Break => self.next(),
                }
            }
        }
    }
}
