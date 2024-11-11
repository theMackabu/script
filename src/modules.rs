pub mod compress;
pub mod file;
pub mod http;
pub mod parse;
pub mod response;
pub mod shell;

pub mod prelude {
    pub use super::compress::*;
    pub use super::file::*;
    pub use super::http::*;
    pub use super::parse::*;
    pub use super::response::*;
    pub use super::shell::*;
    pub use super::Modules;
    pub use crate::database::*;
}

use rhai::{packages::Package, Engine, Module};
use rhai_fs::FilesystemPackage;
use rhai_url::UrlPackage;
use std::{rc::Rc, vec::IntoIter};

type External<'e> = (&'e str, Rc<Module>);

pub struct Modules<'m> {
    pub fs: FilesystemPackage,
    pub url: UrlPackage,
    ext: Vec<External<'m>>,
}

impl<'m> Modules<'m> {
    pub fn new() -> Self {
        Self {
            fs: FilesystemPackage::new(),
            url: UrlPackage::new(),
            ext: vec![],
        }
    }

    pub fn builtin(&self, engine: &mut Engine) {
        self.fs.register_into_engine(engine);
        self.url.register_into_engine(engine);
    }

    pub fn get_ext(self) -> IntoIter<External<'m>> { self.ext.into_iter() }

    pub fn register(&mut self, name: &'m str, module: Module) { self.ext.push((name, module.into())) }
}
