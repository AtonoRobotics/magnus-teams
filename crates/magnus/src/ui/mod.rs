//! Rich answers. Most turns are text. Anything richer is a short line-based
//! spec, written in a fenced block of the answer, that the client compiles
//! against a versioned catalog of elements and draws with its own code.
//! Unknown elements are refused, not guessed.
pub mod apps;
pub mod catalog;
pub mod deck;
pub mod docs;
pub mod images;
pub mod routes;
pub mod spec;
