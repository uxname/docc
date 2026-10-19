//! Package engine loads a project (config, schemas, documents) and runs all
//! checks, producing diagnostics.

mod analyze;
mod core;
mod custom;
mod graph;
mod ignore;
mod kinds;
mod linkrules;
mod lock;
mod mdrules;
mod project;
mod run;
mod strings;
mod textrules;
mod trace;

pub use analyze::{IncUse, RefUse, string_list};
pub use graph::{IMAGE_EXT, image_ext, image_path};
pub use ignore::Ignore;
pub use linkrules::fingerprint;
pub use lock::{LOCK_FILE, Link};
pub use project::{Doc, Project, UsageError, load, valid_id};
pub use run::structural;
pub use strings::{
    INPUT_KEY, INPUT_PROSE, SECTIONS_KEY, SERVICE_FIELDS, StrNode, anchor, anchor_name, heading_anchor,
    is_input, is_sections, resolve, split_anchor,
};

#[cfg(test)]
mod tests;
