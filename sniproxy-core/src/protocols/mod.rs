//! Protocol-specific handlers for web protocols
//!
//! This module contains detection and handling logic for various web protocols:
//! - Socket.IO (Engine.IO v3/v4)
//! - JSON-RPC (1.0/2.0)
//! - XML-RPC
//! - SOAP (1.1/1.2)
//! - Generic RPC over HTTP

pub mod jsonrpc;
pub mod rpc;
pub mod soap;
pub mod socketio;
pub mod xmlrpc;

pub use jsonrpc::*;
pub use rpc::*;
pub use soap::*;
pub use socketio::*;
pub use xmlrpc::*;
