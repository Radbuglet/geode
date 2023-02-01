use std::ops;

use crate::Entity;

// === Mapper === //

pub trait RefMapper<I: ?Sized> {
	type Out: ?Sized;

	fn map_ref<'r>(&self, i: &'r I) -> &'r Self::Out;
}

impl<I, O, F> RefMapper<I> for F
where
	I: ?Sized,
	O: ?Sized,
	F: Fn(&I) -> &O,
{
	type Out = O;

	fn map_ref<'r>(&self, v: &'r I) -> &'r Self::Out {
		(self)(v)
	}
}

pub type FnPtrMapper<A, B> = (fn(&A) -> &B, fn(&mut A) -> &mut B);

pub trait MutMapper<I: ?Sized>: RefMapper<I> {
	fn map_mut<'r>(&self, v: &'r mut I) -> &'r mut Self::Out;
}

impl<I, O, F1, F2> RefMapper<I> for (F1, F2)
where
	I: ?Sized,
	O: ?Sized,
	F1: Fn(&I) -> &O,
{
	type Out = O;

	fn map_ref<'r>(&self, v: &'r I) -> &'r Self::Out {
		(self.0)(v)
	}
}

impl<I, O, F1, F2> MutMapper<I> for (F1, F2)
where
	I: ?Sized,
	O: ?Sized,
	F1: Fn(&I) -> &O,
	F2: Fn(&mut I) -> &mut O,
{
	fn map_mut<'r>(&self, v: &'r mut I) -> &'r mut Self::Out {
		(self.1)(v)
	}
}

// === Core traits === //

pub trait BackingStorage {
	type Comp: ?Sized;
}

pub trait StorageView: ops::Index<Entity, Output = Self::Comp> {
	type Comp: ?Sized;

	fn get(&self, entity: Entity) -> Option<&Self::Comp>;

	fn has(&self, entity: Entity) -> bool;

	fn map_ref<M: RefMapper<Self::Comp>>(&self, mapper: M) -> MappedStorageRef<'_, Self, M> {
		MappedStorageRef {
			storage: self,
			mapper,
		}
	}
}

pub trait StorageViewMut: StorageView + ops::IndexMut<Entity, Output = Self::Comp> {
	fn get_mut(&mut self, entity: Entity) -> Option<&mut Self::Comp>;

	fn map_mut<M: MutMapper<Self::Comp>>(&mut self, mapper: M) -> MappedStorageMut<'_, Self, M> {
		MappedStorageMut {
			storage: self,
			mapper,
		}
	}
}

#[derive(Debug)]
pub struct MappedStorageRef<'a, S: ?Sized, M> {
	pub storage: &'a S,
	pub mapper: M,
}

impl<'a, S, M> ops::Index<Entity> for MappedStorageRef<'a, S, M>
where
	S: ?Sized + StorageView,
	M: RefMapper<S::Comp>,
{
	type Output = M::Out;

	fn index(&self, entity: Entity) -> &Self::Output {
		self.mapper.map_ref(&self.storage[entity])
	}
}

impl<'a, S, M> StorageView for MappedStorageRef<'a, S, M>
where
	S: ?Sized + StorageView,
	M: RefMapper<S::Comp>,
{
	type Comp = M::Out;

	fn get(&self, entity: Entity) -> Option<&Self::Comp> {
		self.storage.get(entity).map(|v| self.mapper.map_ref(v))
	}

	fn has(&self, entity: Entity) -> bool {
		self.storage.has(entity)
	}
}

#[derive(Debug)]
pub struct MappedStorageMut<'a, S: ?Sized, M> {
	pub storage: &'a mut S,
	pub mapper: M,
}

impl<'a, S, M> ops::Index<Entity> for MappedStorageMut<'a, S, M>
where
	S: ?Sized + StorageView,
	M: RefMapper<S::Comp>,
{
	type Output = M::Out;

	fn index(&self, entity: Entity) -> &Self::Output {
		self.mapper.map_ref(&self.storage[entity])
	}
}

impl<'a, S, M> ops::IndexMut<Entity> for MappedStorageMut<'a, S, M>
where
	S: ?Sized + StorageViewMut,
	M: MutMapper<S::Comp>,
{
	fn index_mut(&mut self, entity: Entity) -> &mut Self::Output {
		self.mapper.map_mut(&mut self.storage[entity])
	}
}

impl<'a, S, M> StorageView for MappedStorageMut<'a, S, M>
where
	S: ?Sized + StorageView,
	M: RefMapper<S::Comp>,
{
	type Comp = M::Out;

	fn get(&self, entity: Entity) -> Option<&Self::Comp> {
		self.storage.get(entity).map(|v| self.mapper.map_ref(v))
	}

	fn has(&self, entity: Entity) -> bool {
		self.storage.has(entity)
	}
}

impl<'a, S, M> StorageViewMut for MappedStorageMut<'a, S, M>
where
	S: ?Sized + StorageViewMut,
	M: MutMapper<S::Comp>,
{
	fn get_mut(&mut self, entity: Entity) -> Option<&mut Self::Comp> {
		self.storage.get_mut(entity).map(|v| self.mapper.map_mut(v))
	}
}
