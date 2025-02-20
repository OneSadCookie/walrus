//! Traits and code for emitting high-level structures as low-level, raw wasm
//! structures. E.g. translating from globally unique identifiers down to the
//! raw wasm structure's index spaces.

use crate::encode::{Encoder, MAX_U32_LENGTH};
use crate::ir::Local;
use crate::map::{IdHashMap, IdHashSet};
use crate::{Data, DataId, Element, ElementId, Function, FunctionId};
use crate::{Global, GlobalId, Memory, MemoryId, Module, Table, TableId};
use crate::{Type, TypeId};
use std::ops::{Deref, DerefMut};

pub struct EmitContext<'a> {
    pub module: &'a Module,
    pub indices: &'a mut IdsToIndices,
    pub encoder: Encoder<'a>,
    pub locals: IdHashMap<Function, IdHashSet<Local>>,
}

pub struct SubContext<'a, 'cx> {
    cx: &'cx mut EmitContext<'a>,
    write_size_to: usize,
}

/// Anything that can be lowered to raw wasm structures.
pub trait Emit {
    /// Emit `self` into the given context.
    fn emit(&self, cx: &mut EmitContext);
}

impl<'a, T: ?Sized + Emit> Emit for &'a T {
    fn emit(&self, cx: &mut EmitContext) {
        T::emit(self, cx)
    }
}

/// Maps our high-level identifiers to the raw indices they end up emitted at.
///
/// As we lower to raw wasm structures, we cement various constructs' locations
/// in their respective index spaces. For example, a type with some id `A` ends
/// up being the `i^th` type emitted in the raw wasm type section. When a
/// function references that type, it needs to reference it by its `i` index
/// since the identifier `A` doesn't exist at the raw wasm level.
#[derive(Debug, Default)]
pub struct IdsToIndices {
    tables: IdHashMap<Table, u32>,
    types: IdHashMap<Type, u32>,
    funcs: IdHashMap<Function, u32>,
    globals: IdHashMap<Global, u32>,
    memories: IdHashMap<Memory, u32>,
    elements: IdHashMap<Element, u32>,
    data: IdHashMap<Data, u32>,
    pub(crate) locals: IdHashMap<Function, IdHashMap<Local, u32>>,
}

macro_rules! define_get_index {
    ( $(
        $get_name:ident, $id_ty:ty, $member:ident;
    )* ) => {
        impl IdsToIndices {
            $(
                /// Get the index for the given identifier.
                #[inline]
                pub fn $get_name(&self, id: $id_ty) -> u32 {
                    self.$member.get(&id).cloned().expect(
                        "Should never try and get the index for an identifier that has not already had \
                         its index set. This means that either we are attempting to get the index of \
                         an unused identifier, or that we are emitting sections in the wrong order."
                    )
                }
            )*
        }
    };
}

macro_rules! define_get_push_index {
    ( $(
        $get_name:ident, $push_name:ident, $id_ty:ty, $member:ident;
    )* ) => {
        define_get_index!( $( $get_name, $id_ty, $member; )* );
        impl IdsToIndices {
            $(
                /// Adds the given identifier to this set, assigning it the next
                /// available index.
                #[inline]
                pub(crate) fn $push_name(&mut self, id: $id_ty) {
                    let idx = self.$member.len() as u32;
                    self.$member.insert(id, idx);
                }
            )*
        }
    };
}

define_get_push_index! {
    get_table_index, push_table, TableId, tables;
    get_type_index, push_type, TypeId, types;
    get_func_index, push_func, FunctionId, funcs;
    get_global_index, push_global, GlobalId, globals;
    get_memory_index, push_memory, MemoryId, memories;
    get_element_index, push_element, ElementId, elements;
}
define_get_index! {
    get_data_index, DataId, data;
}

impl IdsToIndices {
    /// Sets the data index to the specified value
    pub(crate) fn set_data_index(&mut self, id: DataId, idx: u32) {
        self.data.insert(id, idx);
    }
}

impl<'a> EmitContext<'a> {
    pub fn start_section<'b>(&'b mut self, id: Section) -> SubContext<'a, 'b> {
        self.subsection(id as u8)
    }

    pub fn subsection<'b>(&'b mut self, id: u8) -> SubContext<'a, 'b> {
        self.encoder.byte(id);
        let start = self.encoder.reserve_u32();
        SubContext {
            cx: self,
            write_size_to: start,
        }
    }

    pub fn custom_section<'b>(&'b mut self, name: &str) -> SubContext<'a, 'b> {
        let mut cx = self.start_section(Section::Custom);
        cx.encoder.str(name);
        return cx;
    }

    pub fn list<T>(&mut self, list: T)
    where
        T: IntoIterator,
        T::IntoIter: ExactSizeIterator,
        T::Item: Emit,
    {
        let list = list.into_iter();
        self.encoder.usize(list.len());
        for item in list {
            item.emit(self);
        }
    }
}

impl<'a> Deref for SubContext<'a, '_> {
    type Target = EmitContext<'a>;

    fn deref(&self) -> &EmitContext<'a> {
        &self.cx
    }
}

impl<'a> DerefMut for SubContext<'a, '_> {
    fn deref_mut(&mut self) -> &mut EmitContext<'a> {
        &mut self.cx
    }
}

impl Drop for SubContext<'_, '_> {
    fn drop(&mut self) {
        let amt = self.cx.encoder.pos() - self.write_size_to - MAX_U32_LENGTH;
        assert!(amt <= u32::max_value() as usize);
        self.cx.encoder.u32_at(self.write_size_to, amt as u32);
    }
}

pub enum Section {
    Custom = 0,
    Type = 1,
    Import = 2,
    Function = 3,
    Table = 4,
    Memory = 5,
    Global = 6,
    Export = 7,
    Start = 8,
    Element = 9,
    Code = 10,
    Data = 11,
    DataCount = 12,
}
