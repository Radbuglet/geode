use std::ops::{self, Deref, DerefMut};

use crate::Entity;

use super::wrapper::CompLocation;

// === Mapper === //

pub type FnPtrMapper<A, B> = (fn(&A) -> &B, fn(&mut A) -> &mut B);

pub trait RefMapper<I: ?Sized> {
	type Out: ?Sized;

	fn map_ref<'r>(&self, v: &'r I) -> &'r Self::Out
	where
		Self: 'r;
}

impl<I, O, F> RefMapper<I> for F
where
	I: ?Sized,
	O: ?Sized,
	F: Fn(&I) -> &O,
{
	type Out = O;

	fn map_ref<'r>(&self, v: &'r I) -> &'r Self::Out
	where
		Self: 'r,
	{
		(self)(v)
	}
}

pub trait MutMapper<I: ?Sized>: RefMapper<I> {
	fn map_mut<'r>(&self, v: &'r mut I) -> &'r mut Self::Out
	where
		Self: 'r;
}

impl<I, O, F1, F2> RefMapper<I> for (F1, F2)
where
	I: ?Sized,
	O: ?Sized,
	F1: Fn(&I) -> &O,
{
	type Out = O;

	fn map_ref<'r>(&self, v: &'r I) -> &'r Self::Out
	where
		Self: 'r,
	{
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
	fn map_mut<'r>(&self, v: &'r mut I) -> &'r mut Self::Out
	where
		Self: 'r,
	{
		(self.1)(v)
	}
}

// === MappedStorage === //

#[derive(Debug, Copy, Clone)]
pub struct MappedStorage<S, M> {
	pub storage: S,
	pub mapper: M,
}

// === StorageView === //

pub trait StorageView: ops::Index<Entity, Output = Self::Comp> {
	type Comp: ?Sized;

	fn get(&self, entity: Entity) -> Option<&Self::Comp>;

	fn has(&self, entity: Entity) -> bool;

	fn map<M>(&self, mapper: M) -> MappedStorage<&Self, M>
	where
		M: RefMapper<Self::Comp>,
	{
		MappedStorage {
			storage: self,
			mapper,
		}
	}
}

pub trait StorageViewMut: StorageView + ops::IndexMut<Entity, Output = Self::Comp> {
	fn get_mut(&mut self, entity: Entity) -> Option<&mut Self::Comp>;

	fn map_mut<M>(&mut self, mapper: M) -> MappedStorage<&mut Self, M>
	where
		M: MutMapper<Self::Comp>,
	{
		MappedStorage {
			storage: self,
			mapper,
		}
	}
}

impl<S, M> ops::Index<Entity> for MappedStorage<S, M>
where
	S: Deref,
	S::Target: StorageView,
	M: RefMapper<<S::Target as StorageView>::Comp>,
{
	type Output = M::Out;

	fn index(&self, index: Entity) -> &Self::Output {
		self.mapper.map_ref(&self.storage[index])
	}
}

impl<S, M> StorageView for MappedStorage<S, M>
where
	S: Deref,
	S::Target: StorageView,
	M: RefMapper<<S::Target as StorageView>::Comp>,
{
	type Comp = M::Out;

	fn get(&self, entity: Entity) -> Option<&Self::Comp> {
		self.storage.get(entity).map(|v| self.mapper.map_ref(v))
	}

	fn has(&self, entity: Entity) -> bool {
		self.storage.has(entity)
	}
}

impl<S, M> ops::IndexMut<Entity> for MappedStorage<S, M>
where
	S: DerefMut,
	S::Target: StorageViewMut,
	M: MutMapper<<S::Target as StorageView>::Comp>,
{
	fn index_mut(&mut self, index: Entity) -> &mut Self::Output {
		self.mapper.map_mut(&mut self.storage[index])
	}
}

impl<S, M> StorageViewMut for MappedStorage<S, M>
where
	S: DerefMut,
	S::Target: StorageViewMut,
	M: MutMapper<<S::Target as StorageView>::Comp>,
{
	fn get_mut(&mut self, entity: Entity) -> Option<&mut Self::Comp> {
		self.storage.get_mut(entity).map(|v| self.mapper.map_mut(v))
	}
}

// === LocatedStorageView === //

pub trait LocatedStorageView<'a>:
	StorageView + ops::Index<CompLocation<'a, Self::BackingComp>, Output = Self::Comp>
{
	type BackingComp: 'a;

	fn locate(&self, entity: Entity) -> CompLocation<'a, Self::BackingComp>;
}

pub trait LocatedStorageViewMut<'a>:
	LocatedStorageView<'a> + ops::IndexMut<CompLocation<'a, Self::BackingComp>, Output = Self::Comp>
{
}

impl<'a, S, M, B> ops::Index<CompLocation<'a, B>> for MappedStorage<S, M>
where
	S: Deref,
	S::Target: LocatedStorageView<'a, BackingComp = B>,
	M: RefMapper<<S::Target as StorageView>::Comp>,
{
	type Output = M::Out;

	fn index(&self, loc: CompLocation<'a, B>) -> &Self::Output {
		self.mapper.map_ref(&self.storage[loc])
	}
}

impl<'a, S, M> LocatedStorageView<'a> for MappedStorage<S, M>
where
	S: Deref,
	S::Target: LocatedStorageView<'a>,
	M: RefMapper<<S::Target as StorageView>::Comp>,
{
	type BackingComp = <S::Target as LocatedStorageView<'a>>::BackingComp;

	fn locate(&self, entity: Entity) -> CompLocation<'a, Self::BackingComp> {
		self.storage.locate(entity)
	}
}

impl<'a, S, M, B> ops::IndexMut<CompLocation<'a, B>> for MappedStorage<S, M>
where
	S: DerefMut,
	S::Target: LocatedStorageViewMut<'a, BackingComp = B>,
	M: MutMapper<<S::Target as StorageView>::Comp>,
{
	fn index_mut(&mut self, loc: CompLocation<'a, B>) -> &mut Self::Output {
		self.mapper.map_mut(&mut self.storage[loc])
	}
}

impl<'a, S, M> LocatedStorageViewMut<'a> for MappedStorage<S, M>
where
	S: DerefMut,
	S::Target: LocatedStorageViewMut<'a>,
	M: MutMapper<<S::Target as StorageView>::Comp>,
{
}
