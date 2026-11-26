//! Persistence layer.
//!
//! `wireproxy.db` is the canonical store for application-owned data. Engine
//! configuration files are materialized from it because external processes
//! (`wireproxy.exe`, `xray.exe`, `tgws-proxy.exe`, Zapret scripts) can only
//! consume files.
pub mod legacy;
pub mod materialize;
