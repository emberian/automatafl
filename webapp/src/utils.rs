// Utility functions for the webapp

#![allow(dead_code)] // Utility methods are part of the public API

use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;

/// Debounce a function call - only execute after delay with no new calls
pub struct Debouncer {
    timeout_id: Rc<RefCell<Option<i32>>>,
    delay_ms: u32,
    // Store the callback to prevent it from being dropped
    _callback: Rc<RefCell<Option<Closure<dyn FnMut()>>>>,
}

impl Debouncer {
    pub fn new(delay_ms: u32) -> Self {
        Self {
            timeout_id: Rc::new(RefCell::new(None)),
            delay_ms,
            _callback: Rc::new(RefCell::new(None)),
        }
    }

    pub fn debounce<F>(&self, f: F)
    where
        F: Fn() + 'static,
    {
        // Clear existing timeout
        if let Some(id) = self.timeout_id.borrow_mut().take() {
            let window = web_sys::window().expect("window should exist");
            window.clear_timeout_with_handle(id);
        }

        // Set new timeout
        let window = web_sys::window().expect("window should exist");
        let timeout_id_clone = self.timeout_id.clone();

        // Use Closure<dyn FnMut()> instead of Closure::once to keep it alive
        let callback = Closure::wrap(Box::new(move || {
            f();
            *timeout_id_clone.borrow_mut() = None;
        }) as Box<dyn FnMut()>);

        let id = window
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                callback.as_ref().unchecked_ref(),
                self.delay_ms as i32,
            )
            .expect("set_timeout should work");

        *self.timeout_id.borrow_mut() = Some(id);

        // Store the callback to keep it alive (replaces the old one, dropping it)
        *self._callback.borrow_mut() = Some(callback);
    }
}

impl Drop for Debouncer {
    fn drop(&mut self) {
        // Clear any pending timeout when the debouncer is dropped
        if let Some(id) = self.timeout_id.borrow_mut().take() {
            if let Some(window) = web_sys::window() {
                window.clear_timeout_with_handle(id);
            }
        }
    }
}

/// Throttle a function call - limit execution to once per interval
pub struct Throttler {
    last_call: Rc<RefCell<Option<f64>>>,
    interval_ms: f64,
}

impl Throttler {
    pub fn new(interval_ms: u32) -> Self {
        Self {
            last_call: Rc::new(RefCell::new(None)),
            interval_ms: interval_ms as f64,
        }
    }

    pub fn throttle<F>(&self, f: F) -> bool
    where
        F: Fn(),
    {
        let now = js_sys::Date::now();
        let mut last_call = self.last_call.borrow_mut();

        let should_call = match *last_call {
            None => true,
            Some(last) => (now - last) >= self.interval_ms,
        };

        if should_call {
            f();
            *last_call = Some(now);
            true
        } else {
            false
        }
    }
}

/// Rate limiter - allows N calls per time window
pub struct RateLimiter {
    calls: Rc<RefCell<Vec<f64>>>,
    max_calls: usize,
    window_ms: f64,
}

impl RateLimiter {
    pub fn new(max_calls: usize, window_ms: u32) -> Self {
        Self {
            calls: Rc::new(RefCell::new(Vec::new())),
            max_calls,
            window_ms: window_ms as f64,
        }
    }

    pub fn allow(&self) -> bool {
        let now = js_sys::Date::now();
        let mut calls = self.calls.borrow_mut();

        // Remove old calls outside the window
        calls.retain(|&call_time| (now - call_time) < self.window_ms);

        // Check if we're under the limit
        if calls.len() < self.max_calls {
            calls.push(now);
            true
        } else {
            false
        }
    }

    pub fn remaining(&self) -> usize {
        let now = js_sys::Date::now();
        let mut calls = self.calls.borrow_mut();

        // Remove old calls outside the window
        calls.retain(|&call_time| (now - call_time) < self.window_ms);

        self.max_calls.saturating_sub(calls.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limiter() {
        let limiter = RateLimiter::new(3, 1000);

        // First 3 calls should succeed
        assert!(limiter.allow());
        assert!(limiter.allow());
        assert!(limiter.allow());

        // 4th call should fail
        assert!(!limiter.allow());

        // Check remaining
        assert_eq!(limiter.remaining(), 0);
    }
}
