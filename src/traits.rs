use generational_arena::Arena;

use crate::vm::Object;

pub trait DisplayArena: Sized {
    type Type;
    fn display_arena(&self, f: &mut dyn std::io::Write, arena: &Arena<Object<Self::Type>>) -> std::io::Result<()>;
}

pub trait Tree<T> {
    fn children(&self) -> Vec<T>;
}
