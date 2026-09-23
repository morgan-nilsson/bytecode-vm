//! Every test for the class file parser, in one binary.
//!
//! Keeping the suites in a single integration test target means `cargo test`
//! prints one total and one failure list rather than eleven of each. Each test
//! name carries its suite as a module path, so a failure reads
//! `header::version_max_accepted`, and `cargo test header::` runs just that
//! suite.

#[macro_use]
mod common;

#[path = "suites/annotations.rs"]
mod annotations;
#[path = "suites/attributes.rs"]
mod attributes;
#[path = "suites/constant_pool.rs"]
mod constant_pool;
#[path = "suites/fixtures.rs"]
mod fixtures;
#[path = "suites/header.rs"]
mod header;
#[path = "suites/java_utf.rs"]
mod java_utf;
#[path = "suites/members.rs"]
mod members;
#[path = "suites/module.rs"]
mod module;
#[path = "suites/reader.rs"]
mod reader;
#[path = "suites/smoke.rs"]
mod smoke;
#[path = "suites/stack_map.rs"]
mod stack_map;
