//! UDP4 Protocol Module
//!
//! This module provides high-level interfaces for working with the UEFI UDP4 protocol.
//! It includes functionality for managing events and protocol operations.

/// Managed event module
///
/// This module provides a safe wrapper around UEFI events that ensures proper cleanup
/// of event resources and associated closures.
pub mod managed_event;

/// Protocol module
///
/// This module provides the core (safe) UDP4 protocol interface, including methods for
/// configuring, transmitting, and receiving UDP packets.
pub mod proto;
