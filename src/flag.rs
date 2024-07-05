// Copyright 2018-2024 the Deno authors. MIT license.

use std::cell::Cell;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

/// A flag with interior mutability that can be raised or lowered.
/// Useful for indicating if an event has occurred.
#[derive(Debug, Default)]
pub struct Flag(Cell<bool>);

impl Flag {
  /// Creates a new flag that's raised.
  pub fn raised() -> Self {
    Self(Cell::new(true))
  }

  /// Raises the flag returning if raised.
  pub fn raise(&self) -> bool {
    !self.0.replace(true)
  }

  /// Lowers the flag returning if lowered.
  pub fn lower(&self) -> bool {
    self.0.replace(false)
  }

  /// Gets if the flag is raised.
  pub fn is_raised(&self) -> bool {
    self.0.get()
  }
}

/// Simplifies the use of an atomic boolean as a flag.
#[derive(Debug, Default)]
pub struct AtomicFlag(AtomicBool);

impl AtomicFlag {
  /// Creates a new flag that's raised.
  pub fn raised() -> AtomicFlag {
    Self(AtomicBool::new(true))
  }

  /// Raises the flag returning if the raise was successful.
  pub fn raise(&self) -> bool {
    !self.0.swap(true, Ordering::SeqCst)
  }

  /// Lowers the flag returning if the lower was successful.
  pub fn lower(&self) -> bool {
    self.0.swap(false, Ordering::SeqCst)
  }

  /// Gets if the flag is raised.
  pub fn is_raised(&self) -> bool {
    self.0.load(Ordering::SeqCst)
  }
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn test_raise_lower() {
    let flag = Flag::default();
    assert!(!flag.is_raised());
    assert!(flag.raise());
    assert!(flag.is_raised());
    assert!(!flag.raise());
    assert!(flag.is_raised());
    assert!(flag.lower());
    assert!(!flag.is_raised());
    assert!(!flag.lower());
    assert!(!flag.is_raised());
  }

  #[test]
  fn atomic_flag_raises_lowers() {
    let flag = AtomicFlag::default();
    assert!(!flag.is_raised()); // false by default
    assert!(flag.raise());
    assert!(flag.is_raised());
    assert!(!flag.raise());
    assert!(flag.is_raised());
    assert!(flag.lower());
    assert!(flag.raise());
    assert!(flag.lower());
    assert!(!flag.lower());
    let flag = AtomicFlag::raised();
    assert!(flag.is_raised());
    assert!(flag.lower());
  }
}
