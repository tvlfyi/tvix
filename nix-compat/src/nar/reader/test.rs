use std::io::Read;

use crate::nar;

#[test]
fn symlink() {
    let mut f = std::io::Cursor::new(include_bytes!("../tests/symlink.nar"));
    let node = nar::reader::open(&mut f).unwrap();

    match node {
        nar::reader::Node::Symlink { target } => {
            assert_eq!(
                &b"/nix/store/somewhereelse"[..],
                &target,
                "target must match"
            );
        }
        _ => panic!("unexpected type"),
    }
}

#[test]
fn file() {
    let mut f = std::io::Cursor::new(include_bytes!("../tests/helloworld.nar"));
    let node = nar::reader::open(&mut f).unwrap();

    match node {
        nar::reader::Node::File {
            executable,
            mut reader,
        } => {
            assert!(!executable);
            let mut buf = vec![];
            reader.read_to_end(&mut buf).expect("read must succeed");
            assert_eq!(&b"Hello World!"[..], &buf);
        }
        _ => panic!("unexpected type"),
    }
}

#[test]
fn complicated() {
    let mut f = std::io::Cursor::new(include_bytes!("../tests/complicated.nar"));
    let node = nar::reader::open(&mut f).unwrap();

    match node {
        nar::reader::Node::Directory(mut dir_reader) => {
            // first entry is .keep, an empty regular file.
            let entry = dir_reader
                .next()
                .expect("next must succeed")
                .expect("must be some");

            assert_eq!(&b".keep"[..], &entry.name);

            match entry.node {
                nar::reader::Node::File {
                    executable,
                    mut reader,
                } => {
                    assert!(!executable);
                    assert_eq!(reader.read(&mut [0]).unwrap(), 0);
                }
                _ => panic!("unexpected type for .keep"),
            }

            // second entry is aa, a symlink to /nix/store/somewhereelse
            let entry = dir_reader
                .next()
                .expect("next must be some")
                .expect("must be some");

            assert_eq!(&b"aa"[..], &entry.name);

            match entry.node {
                nar::reader::Node::Symlink { target } => {
                    assert_eq!(&b"/nix/store/somewhereelse"[..], &target);
                }
                _ => panic!("unexpected type for aa"),
            }

            // third entry is a directory called "keep"
            let entry = dir_reader
                .next()
                .expect("next must be some")
                .expect("must be some");

            assert_eq!(&b"keep"[..], &entry.name);

            match entry.node {
                nar::reader::Node::Directory(mut subdir_reader) => {
                    // first entry is .keep, an empty regular file.
                    let entry = subdir_reader
                        .next()
                        .expect("next must succeed")
                        .expect("must be some");

                    // … it contains a single .keep, an empty regular file.
                    assert_eq!(&b".keep"[..], &entry.name);

                    match entry.node {
                        nar::reader::Node::File {
                            executable,
                            mut reader,
                        } => {
                            assert!(!executable);
                            assert_eq!(reader.read(&mut [0]).unwrap(), 0);
                        }
                        _ => panic!("unexpected type for keep/.keep"),
                    }
                }
                _ => panic!("unexpected type for keep/.keep"),
            }

            // reading more entries yields None (and we actually must read until this)
            assert!(dir_reader.next().expect("must succeed").is_none());
        }
        _ => panic!("unexpected type"),
    }
}
