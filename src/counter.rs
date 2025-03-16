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
