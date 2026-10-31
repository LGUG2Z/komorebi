//! The wire types shared by every komorebi daemon and client.
//!
//! The crate holds no platform code: `SocketMessage` and the payload types it carries,
//! the support table that says which platform handles which message, and the small
//! leaf modules (`animation`, `pathext`) those payloads depend on. The socket
//! transport is not part of it.
#![warn(clippy::all)]
#![allow(clippy::missing_errors_doc, clippy::use_self, clippy::doc_markdown)]

pub mod animation;
pub mod message;
pub mod pathext;
pub mod platforms;
pub mod types;
