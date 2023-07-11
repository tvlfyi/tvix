# Regression test for a bug where tvix would crash in nix_escape_string
# because it counted the string position by unicode code point count,
# but then used it as a byte index for slicing. Consequently, it would
# try slicing 💭 in half, thinking the first element to be escaped was
# at byte index 2 (i.e. the quote).
"💭(\":thonking:\")"
