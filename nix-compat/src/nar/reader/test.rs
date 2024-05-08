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
            must_read_file(
                ".keep",
                dir_reader
                    .next()
                    .expect("next must succeed")
                    .expect("must be some"),
            );

            // second entry is aa, a symlink to /nix/store/somewhereelse
            must_be_symlink(
                "aa",
                "/nix/store/somewhereelse",
                dir_reader
                    .next()
                    .expect("next must be some")
                    .expect("must be some"),
            );

            {
                // third entry is a directory called "keep"
                let entry = dir_reader
                    .next()
                    .expect("next must be some")
                    .expect("must be some");

                assert_eq!(b"keep", entry.name);

                match entry.node {
                    nar::reader::Node::Directory(mut subdir_reader) => {
                        {
                            // first entry is .keep, an empty regular file.
                            let entry = subdir_reader
                                .next()
                                .expect("next must succeed")
                                .expect("must be some");

                            must_read_file(".keep", entry);
                        }

                        // we must read the None
                        assert!(
                            subdir_reader.next().expect("next must succeed").is_none(),
                            "keep directory contains only .keep"
                        );
                    }
                    _ => panic!("unexpected type for keep/.keep"),
                }
            };

            // reading more entries yields None (and we actually must read until this)
            assert!(dir_reader.next().expect("must succeed").is_none());
        }
        _ => panic!("unexpected type"),
    }
}

#[test]
#[should_panic]
fn file_read_abandoned() {
    let mut f = std::io::Cursor::new(include_bytes!("../tests/complicated.nar"));
    let node = nar::reader::open(&mut f).unwrap();

    match node {
        nar::reader::Node::Directory(mut dir_reader) => {
            // first entry is .keep, an empty regular file.
            {
                let entry = dir_reader
                    .next()
                    .expect("next must succeed")
                    .expect("must be some");

                assert_eq!(b".keep", entry.name);
                // don't bother to finish reading it.
            };

            // this should panic (not return an error), because we are meant to abandon the archive reader now.
            assert!(dir_reader.next().expect("must succeed").is_none());
        }
        _ => panic!("unexpected type"),
    }
}

#[test]
#[should_panic]
fn dir_read_abandoned() {
    let mut f = std::io::Cursor::new(include_bytes!("../tests/complicated.nar"));
    let node = nar::reader::open(&mut f).unwrap();

    match node {
        nar::reader::Node::Directory(mut dir_reader) => {
            // first entry is .keep, an empty regular file.
            must_read_file(
                ".keep",
                dir_reader
                    .next()
                    .expect("next must succeed")
                    .expect("must be some"),
            );

            // second entry is aa, a symlink to /nix/store/somewhereelse
            must_be_symlink(
                "aa",
                "/nix/store/somewhereelse",
                dir_reader
                    .next()
                    .expect("next must be some")
                    .expect("must be some"),
            );

            {
                // third entry is a directory called "keep"
                let entry = dir_reader
                    .next()
                    .expect("next must be some")
                    .expect("must be some");

                assert_eq!(b"keep", entry.name);

                match entry.node {
                    nar::reader::Node::Directory(_) => {
                        // don't finish using it, which poisons the archive reader
                    }
                    _ => panic!("unexpected type for keep/.keep"),
                }
            };

            // this should panic, because we didn't finish reading the child subdirectory
            assert!(dir_reader.next().expect("must succeed").is_none());
        }
        _ => panic!("unexpected type"),
    }
}

#[test]
#[should_panic]
fn dir_read_after_none() {
    let mut f = std::io::Cursor::new(include_bytes!("../tests/complicated.nar"));
    let node = nar::reader::open(&mut f).unwrap();

    match node {
        nar::reader::Node::Directory(mut dir_reader) => {
            // first entry is .keep, an empty regular file.
            must_read_file(
                ".keep",
                dir_reader
                    .next()
                    .expect("next must succeed")
                    .expect("must be some"),
            );

            // second entry is aa, a symlink to /nix/store/somewhereelse
            must_be_symlink(
                "aa",
                "/nix/store/somewhereelse",
                dir_reader
                    .next()
                    .expect("next must be some")
                    .expect("must be some"),
            );

            {
                // third entry is a directory called "keep"
                let entry = dir_reader
                    .next()
                    .expect("next must be some")
                    .expect("must be some");

                assert_eq!(b"keep", entry.name);

                match entry.node {
                    nar::reader::Node::Directory(mut subdir_reader) => {
                        // first entry is .keep, an empty regular file.
                        must_read_file(
                            ".keep",
                            subdir_reader
                                .next()
                                .expect("next must succeed")
                                .expect("must be some"),
                        );

                        // we must read the None
                        assert!(
                            subdir_reader.next().expect("next must succeed").is_none(),
                            "keep directory contains only .keep"
                        );
                    }
                    _ => panic!("unexpected type for keep/.keep"),
                }
            };

            // reading more entries yields None (and we actually must read until this)
            assert!(dir_reader.next().expect("must succeed").is_none());

            // this should panic, because we already got a none so we're meant to stop.
            dir_reader.next().unwrap();
            unreachable!()
        }
        _ => panic!("unexpected type"),
    }
}

fn must_read_file(name: &'static str, entry: nar::reader::Entry<'_, '_>) {
    assert_eq!(name.as_bytes(), entry.name);

    match entry.node {
        nar::reader::Node::File {
            executable,
            mut reader,
        } => {
            assert!(!executable);
            assert_eq!(reader.read(&mut [0]).unwrap(), 0);
        }
        _ => panic!("unexpected type for {}", name),
    }
}

fn must_be_symlink(
    name: &'static str,
    exp_target: &'static str,
    entry: nar::reader::Entry<'_, '_>,
) {
    assert_eq!(name.as_bytes(), entry.name);

    match entry.node {
        nar::reader::Node::Symlink { target } => {
            assert_eq!(exp_target.as_bytes(), &target);
        }
        _ => panic!("unexpected type for {}", name),
    }
}
