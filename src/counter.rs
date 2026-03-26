use std::sync::atomic::{AtomicUsize, Ordering};

use log::info;

pub struct Counter
{
  slow_index: AtomicUsize,
  fast_index: AtomicUsize,
}

impl Counter
{
  fn new() -> Self
  {
    Counter {
      fast_index: AtomicUsize::new(0),
      slow_index: AtomicUsize::new(0),
    }
  }

  pub fn instance() -> &'static Counter
  {
    use lazy_static::lazy_static;
    lazy_static! {
      static ref COUNTER: Counter = Counter::new();
    }
    &COUNTER
  }

  pub fn get_fast_index(
    &self,
    fast_array_size: usize,
  ) -> usize
  {
    let current = self.fast_index.fetch_add(1, Ordering::SeqCst);
    if current >= fast_array_size.saturating_sub(1)
    {
      self.fast_index.store(0, Ordering::SeqCst);
    }
    current
  }

  pub fn get_slow_index(
    &self,
    slow_array_size: usize,
    fast_array_size: usize,
  ) -> usize
  {
    // Get current animal index
    let current_fast = self.fast_index.load(Ordering::SeqCst);

    let will_wrap = current_fast + 1 >= fast_array_size;

    let current_index = if will_wrap
    {
      let current = self.slow_index.fetch_add(1, Ordering::SeqCst);

      if current + 1 >= slow_array_size
      {
        self.slow_index.store(0, Ordering::SeqCst);
      }

      current
    }
    else
    {
      self.slow_index.load(Ordering::SeqCst)
    };
    info!("ADJ INdex {current_index}");

    current_index
  }
}

#[cfg(test)]
mod tests
{
  use super::*;

  /// Fast index returns sequential values starting from zero.
  #[test]
  fn test_get_fast_index_returns_sequential_values()
  {
    let counter = Counter::new();
    assert_eq!(counter.get_fast_index(5), 0);
    assert_eq!(counter.get_fast_index(5), 1);
    assert_eq!(counter.get_fast_index(5), 2);
    assert_eq!(counter.get_fast_index(5), 3);
  }

  /// Fast index resets to 0 after reaching the last valid position.
  #[test]
  fn test_get_fast_index_wraps_at_last_element()
  {
    let counter = Counter::new();
    // Advance to the last element (index 4 of 5)
    for _ in 0..4
    {
      counter.get_fast_index(5);
    }
    // Returns 4, then resets the internal index to 0
    assert_eq!(counter.get_fast_index(5), 4);
    // Next call returns 0 because the reset happened
    assert_eq!(counter.get_fast_index(5), 0);
  }

  /// A size-one array always returns 0 and resets immediately.
  #[test]
  fn test_get_fast_index_with_size_one()
  {
    let counter = Counter::new();
    assert_eq!(counter.get_fast_index(1), 0);
    assert_eq!(counter.get_fast_index(1), 0);
    assert_eq!(counter.get_fast_index(1), 0);
  }

  /// Slow index does not change while the fast index is not near wrap.
  #[test]
  fn test_get_slow_index_stable_before_wrap()
  {
    let counter = Counter::new();
    // After get_fast, fast_index is 1; will_wrap = (1+1 >= 5) = false
    counter.get_fast_index(5);
    assert_eq!(counter.get_slow_index(5, 5), 0);
    counter.get_fast_index(5);
    assert_eq!(counter.get_slow_index(5, 5), 0);
    counter.get_fast_index(5);
    assert_eq!(counter.get_slow_index(5, 5), 0);
  }

  /// Slow index increments when fast index is at its last position (about to
  /// wrap).
  #[test]
  fn test_get_slow_index_increments_when_fast_near_wrap()
  {
    let counter = Counter::new();
    // Advance fast to position 4 of 5 (last element)
    for _ in 0..4
    {
      counter.get_fast_index(5);
    }
    // fast_index is now 4; will_wrap = (4+1 >= 5) = true
    // Returns current slow (0) and increments slow to 1
    assert_eq!(counter.get_slow_index(5, 5), 0);
    // Consume the last fast element; fast resets to 0
    counter.get_fast_index(5);
    // fast_index is 0; will_wrap = (0+1 >= 5) = false -> slow stays at 1
    assert_eq!(counter.get_slow_index(5, 5), 1);
  }

  /// Slow index wraps back to 0 after reaching its array boundary.
  #[test]
  fn test_get_slow_index_wraps_around()
  {
    let counter = Counter::new();
    let fast_size = 2;
    let slow_size = 2;

    // Cycle 1: fast 0->1; get_slow sees fast=1, will_wrap=(2>=2)=true -> slow:
    // 0 returned, 1 stored
    counter.get_fast_index(fast_size);
    assert_eq!(counter.get_slow_index(slow_size, fast_size), 0);

    // Consume last fast element; fast resets to 0
    counter.get_fast_index(fast_size);
    // fast=0; will_wrap=(1>=2)=false -> slow=1 returned unchanged
    assert_eq!(counter.get_slow_index(slow_size, fast_size), 1);

    // Cycle 2: fast 0->1; slow is now at boundary (1+1 >= slow_size=2) so it
    // returns 1 and resets to 0 internally.
    counter.get_fast_index(fast_size);
    assert_eq!(counter.get_slow_index(slow_size, fast_size), 1);

    // After reset slow=0; fast is back at 0
    counter.get_fast_index(fast_size);
    assert_eq!(counter.get_slow_index(slow_size, fast_size), 0);
  }
}
