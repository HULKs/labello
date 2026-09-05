//! Shared client contract for Labello frontends.
//!
//! UI crates should depend on these traits instead of hard-coding HTTP,
//! browser storage, or direct filesystem access.

pub mod demo;
pub mod dto;
pub mod error;
pub mod export;
pub mod http;
pub mod import;
pub mod preview;
pub mod traits;

pub use demo::DemoLabelloApi;
pub use dto::*;
pub use error::*;
pub use export::*;
pub use http::HttpLabelloApi;
pub use import::*;
pub use traits::*;

mod build_information;
pub use build_information::{BuildIdentity, BuildInformationApi};
