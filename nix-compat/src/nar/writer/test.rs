use crate::nar;

#[test]
fn symlink() {
    let mut buf = vec![];
    let node = nar::writer::open(&mut buf).unwrap();

    node.symlink("/nix/store/somewhereelse".as_bytes()).unwrap();

    assert_eq!(include_bytes!("../tests/symlink.nar"), buf.as_slice());
}

#[cfg(feature = "async")]
#[test]
fn symlink_async() {
    let mut buf = vec![];

    futures::executor::block_on(async {
        let node = nar::writer::r#async::open(&mut buf).await.unwrap();
        node.symlink("/nix/store/somewhereelse".as_bytes())
            .await
            .unwrap();
    });

    assert_eq!(include_bytes!("../tests/symlink.nar"), buf.as_slice());
}

#[test]
fn file() {
    let mut buf = vec![];
    let node = nar::writer::open(&mut buf).unwrap();

    let file_contents = "Hello World!".to_string();
    node.file(
        false,
        file_contents.len() as u64,
        &mut std::io::Cursor::new(file_contents),
    )
    .unwrap();

    assert_eq!(include_bytes!("../tests/helloworld.nar"), buf.as_slice());
}

#[cfg(feature = "async")]
#[test]
fn file_async() {
    let mut buf = vec![];

    futures::executor::block_on(async {
        let node = nar::writer::r#async::open(&mut buf).await.unwrap();

        let file_contents = "Hello World!".to_string();
        node.file(
            false,
            file_contents.len() as u64,
            &mut futures::io::Cursor::new(file_contents),
        )
        .await
        .unwrap();
    });

    assert_eq!(include_bytes!("../tests/helloworld.nar"), buf.as_slice());
}

#[test]
fn complicated() {
    let mut buf = vec![];
    let node = nar::writer::open(&mut buf).unwrap();

    let mut dir_node = node.directory().unwrap();

    let e = dir_node.entry(".keep".as_bytes()).unwrap();
    e.file(false, 0, &mut std::io::Cursor::new([]))
        .expect("read .keep must succeed");

    let e = dir_node.entry("aa".as_bytes()).unwrap();
    e.symlink("/nix/store/somewhereelse".as_bytes())
        .expect("symlink must succeed");

    let e = dir_node.entry("keep".as_bytes()).unwrap();
    let mut subdir_node = e.directory().expect("directory must succeed");

    let e_sub = subdir_node
        .entry(".keep".as_bytes())
        .expect("subdir entry must succeed");
    e_sub.file(false, 0, &mut std::io::Cursor::new([])).unwrap();

    // close the subdir, and then the dir, which is required.
    subdir_node.close().unwrap();
    dir_node.close().unwrap();

    assert_eq!(include_bytes!("../tests/complicated.nar"), buf.as_slice());
}

#[cfg(feature = "async")]
#[test]
fn complicated_async() {
    let mut buf = vec![];

    futures::executor::block_on(async {
        let node = nar::writer::r#async::open(&mut buf).await.unwrap();

        let mut dir_node = node.directory().await.unwrap();

        let e = dir_node.entry(".keep".as_bytes()).await.unwrap();
        e.file(false, 0, &mut futures::io::Cursor::new([]))
            .await
            .expect("read .keep must succeed");

        let e = dir_node.entry("aa".as_bytes()).await.unwrap();
        e.symlink("/nix/store/somewhereelse".as_bytes())
            .await
            .expect("symlink must succeed");

        let e = dir_node.entry("keep".as_bytes()).await.unwrap();
        let mut subdir_node = e.directory().await.expect("directory must succeed");

        let e_sub = subdir_node
            .entry(".keep".as_bytes())
            .await
            .expect("subdir entry must succeed");
        e_sub
            .file(false, 0, &mut futures::io::Cursor::new([]))
            .await
            .unwrap();

        // close the subdir, and then the dir, which is required.
        subdir_node.close().await.unwrap();
        dir_node.close().await.unwrap();
    });

    assert_eq!(include_bytes!("../tests/complicated.nar"), buf.as_slice());
}
