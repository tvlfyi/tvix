use codemap::CodeMap;

/// Create a dummy [`codemap::Span`] for use in tests
pub(crate) fn dummy_span() -> codemap::Span {
    let mut codemap = CodeMap::new();
    let file = codemap.add_file("<dummy>".to_owned(), "<dummy>".to_owned());
    file.span
}
