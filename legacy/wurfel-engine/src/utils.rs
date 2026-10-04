//! Utility functions and data structures

use std::collections::VecDeque;

/// Simple object pool for reusing expensive-to-create objects
pub struct Pool<T> {
    objects: VecDeque<T>,
    factory: Box<dyn Fn() -> T>,
    max_size: usize,
}

impl<T> Pool<T> {
    /// Create a new pool with a factory function
    pub fn new<F>(factory: F, max_size: usize) -> Self 
    where 
        F: Fn() -> T + 'static 
    {
        Self {
            objects: VecDeque::new(),
            factory: Box::new(factory),
            max_size,
        }
    }
    
    /// Get an object from the pool (creates new if pool is empty)
    pub fn get(&mut self) -> T {
        self.objects.pop_front().unwrap_or_else(|| (self.factory)())
    }
    
    /// Return an object to the pool
    pub fn put(&mut self, object: T) {
        if self.objects.len() < self.max_size {
            self.objects.push_back(object);
        }
        // If pool is full, object is dropped
    }
    
    /// Clear all objects from the pool
    pub fn clear(&mut self) {
        self.objects.clear();
    }
    
    /// Get the current number of pooled objects
    pub fn len(&self) -> usize {
        self.objects.len()
    }
    
    /// Check if the pool is empty
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }
}

impl<T> std::fmt::Debug for Pool<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pool")
            .field("len", &self.objects.len())
            .field("max_size", &self.max_size)
            .finish()
    }
}