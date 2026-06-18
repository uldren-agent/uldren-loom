//! Engine-free decoding and JSON projection of Loom canonical result buffers.
//!
//! The engine-side encoder (which builds these buffers from `Table`/`Row`/`RowDiff`) lives in
//! `loom-sql`; this crate is the inverse and is engine-free, so every consumer shares one decoder: the
//! hosted wire protocols (`pg_wire`, `mysql_wire`), the FFI and language bindings, and both the local
//! and remote Loom clients. Depends only on `loom-codec` and `loom-types`.
//!
//! Licensed under BUSL-1.1.

pub mod bridge_json;
pub mod result_json;
pub mod result_view;
pub mod view;

pub use bridge_json::to_bridge_json;
pub use result_json::result_to_json;
pub use result_view::decode;
