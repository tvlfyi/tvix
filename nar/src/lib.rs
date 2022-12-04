use std::io::{self, BufRead, ErrorKind::UnexpectedEof, Write};

mod wire;

pub type Writer<'a> = dyn Write + 'a;

pub fn open<'a, 'w: 'a>(writer: &'a mut Writer<'w>) -> io::Result<Node<'a, 'w>> {
    let mut node = Node { writer };
    node.write(&wire::TOK_NAR)?;
    Ok(node)
}

pub struct Node<'a, 'w: 'a> {
    writer: &'a mut Writer<'w>,
}

impl<'a, 'w> Node<'a, 'w> {
    fn write(&mut self, data: &[u8]) -> io::Result<()> {
        self.writer.write_all(data)
    }

    fn pad(&mut self, n: u64) -> io::Result<()> {
        match (n & 7) as usize {
            0 => Ok(()),
            n => self.write(&[0; 8][n..]),
        }
    }

    pub fn symlink(mut self, target: &str) -> io::Result<()> {
        debug_assert!(
            target.len() <= wire::MAX_TARGET_LEN,
            "target.len() > {}",
            wire::MAX_TARGET_LEN
        );
        debug_assert!(
            !target.contains('\0'),
            "invalid target characters: {target:?}"
        );
        debug_assert!(!target.is_empty(), "empty target");

        self.write(&wire::TOK_SYM)?;
        self.write(&target.len().to_le_bytes())?;
        self.write(target.as_bytes())?;
        self.pad(target.len() as u64)?;
        self.write(&wire::TOK_PAR)?;
        Ok(())
    }

    pub fn file(mut self, executable: bool, size: u64, reader: &mut dyn BufRead) -> io::Result<()> {
        self.write(if executable {
            &wire::TOK_EXE
        } else {
            &wire::TOK_REG
        })?;

        self.write(&size.to_le_bytes())?;

        let mut need = size;
        while need != 0 {
            let data = reader.fill_buf()?;

            if data.is_empty() {
                return Err(UnexpectedEof.into());
            }

            let n = need.min(data.len() as u64) as usize;
            self.write(&data[..n])?;

            need -= n as u64;
            reader.consume(n);
        }

        self.pad(size)?;
        self.write(&wire::TOK_PAR)?;

        Ok(())
    }

    pub fn directory(mut self) -> io::Result<Directory<'a, 'w>> {
        self.write(&wire::TOK_DIR)?;
        Ok(Directory::new(self))
    }
}

#[cfg(debug_assertions)]
type Name = String;
#[cfg(not(debug_assertions))]
type Name = ();

fn into_name(_name: &str) -> Name {
    #[cfg(debug_assertions)]
    _name.to_owned()
}

pub struct Directory<'a, 'w> {
    node: Node<'a, 'w>,
    prev_name: Option<Name>,
}

impl<'a, 'w> Directory<'a, 'w> {
    fn new(node: Node<'a, 'w>) -> Self {
        Self {
            node,
            prev_name: None,
        }
    }

    pub fn entry(&mut self, name: &str) -> io::Result<Node<'_, 'w>> {
        debug_assert!(
            name.len() <= wire::MAX_NAME_LEN,
            "name.len() > {}",
            wire::MAX_NAME_LEN
        );
        debug_assert!(!["", ".", ".."].contains(&name), "invalid name: {name:?}");
        debug_assert!(
            !name.contains(['/', '\0']),
            "invalid name characters: {name:?}"
        );

        match self.prev_name {
            None => {
                self.prev_name = Some(into_name(name));
            }
            Some(ref mut _prev_name) => {
                #[cfg(debug_assertions)]
                {
                    assert!(
                        &**_prev_name < name,
                        "misordered names: {_prev_name:?} >= {name:?}"
                    );
                    _prev_name.clear();
                    _prev_name.push_str(name);
                }
                self.node.write(&wire::TOK_PAR)?;
            }
        }

        self.node.write(&wire::TOK_ENT)?;
        self.node.write(&name.len().to_le_bytes())?;
        self.node.write(name.as_bytes())?;
        self.node.pad(name.len() as u64)?;
        self.node.write(&wire::TOK_NOD)?;

        Ok(Node {
            writer: &mut *self.node.writer,
        })
    }

    pub fn close(mut self) -> io::Result<()> {
        if self.prev_name.is_some() {
            self.node.write(&wire::TOK_PAR)?;
        }

        self.node.write(&wire::TOK_PAR)?;
        Ok(())
    }
}
