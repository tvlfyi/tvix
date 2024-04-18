/// Protocol versions are represented as a u16.
/// The upper 8 bits are the major version, the lower bits the minor.
/// This is not aware of any endianness, use [crate::wire::read_u64] to get an
/// u64 first, and the try_from() impl from here if you're receiving over the
/// Nix Worker protocol.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProtocolVersion(u16);

impl ProtocolVersion {
    pub const fn from_parts(major: u8, minor: u8) -> Self {
        Self(((major as u16) << 8) | minor as u16)
    }

    pub fn major(&self) -> u8 {
        ((self.0 & 0xff00) >> 8) as u8
    }

    pub fn minor(&self) -> u8 {
        (self.0 & 0x00ff) as u8
    }
}

impl PartialOrd for ProtocolVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ProtocolVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.major().cmp(&other.major()) {
            std::cmp::Ordering::Less => std::cmp::Ordering::Less,
            std::cmp::Ordering::Greater => std::cmp::Ordering::Greater,
            std::cmp::Ordering::Equal => {
                // same major, compare minor
                self.minor().cmp(&other.minor())
            }
        }
    }
}

impl From<u16> for ProtocolVersion {
    fn from(value: u16) -> Self {
        Self::from_parts(((value & 0xff00) >> 8) as u8, (value & 0x00ff) as u8)
    }
}

impl TryFrom<u64> for ProtocolVersion {
    type Error = &'static str;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        if value & !0xffff != 0 {
            return Err("only two least significant bits might be populated");
        }

        Ok((value as u16).into())
    }
}

impl From<ProtocolVersion> for u16 {
    fn from(value: ProtocolVersion) -> Self {
        value.0
    }
}

impl From<ProtocolVersion> for u64 {
    fn from(value: ProtocolVersion) -> Self {
        value.0 as u64
    }
}

impl std::fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major(), self.minor())
    }
}

#[cfg(test)]
mod tests {
    use super::ProtocolVersion;

    #[test]
    fn from_parts() {
        let version = ProtocolVersion::from_parts(1, 37);
        assert_eq!(version.major(), 1, "correct major");
        assert_eq!(version.minor(), 37, "correct minor");
        assert_eq!("1.37", &version.to_string(), "to_string");

        assert_eq!(0x0125, Into::<u16>::into(version));
        assert_eq!(0x0125, Into::<u64>::into(version));
    }

    #[test]
    fn from_u16() {
        let version = ProtocolVersion::from(0x0125_u16);
        assert_eq!("1.37", &version.to_string());
    }

    #[test]
    fn from_u64() {
        let version = ProtocolVersion::try_from(0x0125_u64).expect("must succeed");
        assert_eq!("1.37", &version.to_string());
    }

    /// This contains data in higher bits, which should fail.
    #[test]
    fn from_u64_fail() {
        ProtocolVersion::try_from(0xaa0125_u64).expect_err("must fail");
    }

    #[test]
    fn ord() {
        let v0_37 = ProtocolVersion::from_parts(0, 37);
        let v1_37 = ProtocolVersion::from_parts(1, 37);
        let v1_40 = ProtocolVersion::from_parts(1, 40);

        assert!(v0_37 < v1_37);
        assert!(v1_37 > v0_37);
        assert!(v1_37 < v1_40);
        assert!(v1_40 > v1_37);
        assert!(v1_40 <= v1_40);
    }
}
