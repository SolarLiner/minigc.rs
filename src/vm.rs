use std::{
    fmt::{self},
};
use std::fmt::Debug;

use generational_arena::{Arena, Index};
use tracing::{debug, instrument, trace};


use crate::traits::{DisplayArena, Tree};

#[derive(Copy, Clone, Debug)]
pub struct Object<V> {
    pub(crate) marked: bool,
    pub(crate) index: Index,
    pub(crate) value: V,
}

impl<I, V: Tree<I>> Tree<I> for Object<V> {
    fn children(&self) -> Vec<I> {
        self.value.children()
    }
}

impl<V> Object<V> {
    pub fn new(index: Index, value: V) -> Self {
        Self {
            value,
            index,
            marked: false,
        }
    }
}

impl<V: DisplayArena> DisplayArena for Object<V> {
    type Type = V::Type;
    fn display_arena(&self, f: &mut dyn std::io::Write, arena: &Arena<Object<Self::Type>>) -> std::io::Result<()> {
        self.value.display_arena(f, arena)
    }
}

#[derive(Clone, Debug)]
pub struct VM<V> {
    pub(crate) objects: Arena<Object<V>>,
    pub(crate) stack: Vec<Index>,
    pub(crate) max_objects: usize,
    pub(crate) gc_on: bool,
}

impl<V: DisplayArena<Type=V> + Tree<Index> + Debug> VM<V> {
    pub(crate) fn display(&self, id: Index) -> std::io::Result<impl fmt::Display> {
        let obj = self.get(id);
        let mut s = Vec::new();
        obj.display_arena(&mut s, &self.objects)?;
        Ok(unsafe { String::from_utf8_unchecked(s) })
    }
}

impl<V: Tree<Index> + Debug> VM<V> {
    pub fn new(size: usize) -> Self {
        Self {
            objects: Arena::new(),
            stack: vec![],
            max_objects: size,
            gc_on: true,
        }
    }

    #[instrument(skip(self))]
    pub fn gc(&mut self, on: bool) {
        self.gc_on = on;
        if self.gc_on && self.objects.len() > self.max_objects {
            self.garbage_collect();
        }
    }

    pub fn get(&self, id: Index) -> &Object<V> {
        &self.objects[id]
    }

    pub fn get_mut(&mut self, id: Index) -> &mut Object<V> {
        &mut self.objects[id]
    }

    #[instrument(skip(self))]
    pub fn push_value(&mut self, val: V) {
        let obj = self.objects.insert_with(|idx| {
            debug!(?idx);
            Object::new(idx, val)
        });
        self.push(obj);
        if self.objects.len() > self.max_objects {
            self.garbage_collect();
        }
    }

    #[instrument(skip(self))]
    pub fn garbage_collect(&mut self) {
        if !self.gc_on {
            return;
        }
        trace!("Garbage collection");
        self.mark_all();
        self.sweep();
    }

    #[instrument(skip(self))]
    pub(crate) fn push(&mut self, id: Index) {
        self.stack.push(id);
    }

    pub fn top(&self) -> Option<Index> {
        self.stack.last().copied()
    }

    #[instrument(skip(self))]
    pub fn pop(&mut self) -> Option<Index> {
        let id = self.stack.pop()?;
        Some(id)
    }

    #[instrument(skip(self))]
    fn mark(&mut self, id: Index) {
        let obj = self.get_mut(id);
        obj.marked = true;
        for child in obj.children() {
            self.mark(child);
        }
    }

    #[instrument(skip(self))]
    fn mark_all(&mut self) {
        trace!(stack = ?self.stack, objects = ?self.objects);
        for obj in 0..self.stack.len() {
            self.mark(self.stack[obj]);
        }
    }

    #[instrument(skip(self))]
    fn sweep(&mut self) {
        self.objects.retain(|id, obj| {
            if obj.marked {
                obj.marked = false;
                true
            } else {
                trace!("removing object; id = {:?}", id);
                false
            }
        })
    }
}
