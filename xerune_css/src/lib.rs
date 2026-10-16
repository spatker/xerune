//! The CSS parser shared by `xerune` (runtime stylesheets) and
//! `xerune_derive` (compiled templates), so both read and match a
//! stylesheet the same way. A proc-macro crate can't use the crate it generates code for;
//! both can use this one.

#![no_std]
#![warn(missing_docs)]

extern crate alloc;

pub mod media;
pub mod pseudo;
pub mod selector;
pub mod sheet;
pub mod text;

pub use pseudo::ControlPart;
