use lazy_static::lazy_static;

pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/tvix.flatstore.v1.rs"));
}

lazy_static! {
    static ref DB: sled::Db = sled::open("crunch.db").unwrap();
    pub static ref FILES: sled::Tree = DB.open_tree("files").unwrap();
}
