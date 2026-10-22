//! Package render turns validated documents into Markdown pages (docs/out) and
//! the audit build (docs/out-full).

mod brand;
mod comments;
mod html;
mod htmlparts;
mod lang;
mod page;
#[allow(clippy::module_inception)]
mod render;
mod text;
mod write;

pub use lang::labels;
pub use html::compile::{CBlock, CPage, Compiled, ProjectInfo, SCHEMA_ID as COMPILE_SCHEMA_ID, Section, Seen};
pub use page::block_field;
pub use render::{HEADER, Renderer, page_path};
pub use write::{check_dir, check_fresh, generated_file, single_file, write, write_file};

#[cfg(test)]
mod tests;
