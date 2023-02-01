use std::{
	cell::UnsafeCell,
	ops::{Index, IndexMut},
	sync::atomic::{AtomicU64, Ordering},
};

use derive_where::derive_where;

use crate::{ArchetypeId, Entity, Storage, StorageView, StorageViewMut};

use super::{
	container::StorageRunView,
	view::{LocatedStorageView, LocatedStorageViewMut},
};

pub trait StorageWrapper<'r> {
	type Comp;

	fn wrap(storage: &'r mut Storage<Self::Comp>) -> Self;
}

#[derive(Debug)]
pub struct LocatedStorage<'a, T> {
	storage: &'a Storage<UnsafeCell<T>>,
	key: u64,
}

impl<'a, T> StorageWrapper<'a> for LocatedStorage<'a, T> {
	type Comp = T;

	fn wrap(storage: &'a mut Storage<Self::Comp>) -> Self {
		static KEY_GEN: AtomicU64 = AtomicU64::new(0);

		Self {
			storage: storage.as_celled(),
			key: KEY_GEN.fetch_add(1, Ordering::Relaxed),
		}
	}
}

impl<'a, T> LocatedStorage<'a, T> {
	pub fn try_locate(&self, entity: Entity) -> Option<CompLocation<'a, T>> {
		self.storage.get(entity).map(|value| CompLocation {
			value,
			entity,
			key: self.key,
		})
	}

	pub fn locate(&self, entity: Entity) -> CompLocation<'a, T> {
		CompLocation {
			value: &self.storage[entity],
			entity,
			key: self.key,
		}
	}

	pub fn get_run(&self, archetype: ArchetypeId) -> LocatedStorageRun<'a, T> {
		LocatedStorageRun {
			key: self.key,
			run: self.storage.get_run_view(archetype),
		}
	}

	pub fn get(&self, entity: Entity) -> Option<&T> {
		self.storage.get(entity).map(|v| unsafe { &*v.get() })
	}

	pub fn get_mut(&mut self, entity: Entity) -> Option<&mut T> {
		self.storage.get(entity).map(|v| unsafe { &mut *v.get() })
	}

	pub fn has(&self, entity: Entity) -> bool {
		self.storage.has(entity)
	}
}

impl<'a, 'b: 'a, T> Index<CompLocation<'b, T>> for LocatedStorage<'a, T> {
	type Output = T;

	fn index(&self, loc: CompLocation<'b, T>) -> &Self::Output {
		assert_eq!(self.key, loc.key);

		unsafe { &*loc.value.get() }
	}
}

impl<'a, 'b: 'a, T> IndexMut<CompLocation<'b, T>> for LocatedStorage<'a, T> {
	fn index_mut(&mut self, loc: CompLocation<'b, T>) -> &mut Self::Output {
		assert_eq!(self.key, loc.key);

		unsafe { &mut *loc.value.get() }
	}
}

impl<'a, T> Index<Entity> for LocatedStorage<'a, T> {
	type Output = T;

	fn index(&self, loc: Entity) -> &Self::Output {
		unsafe { &*self.storage[loc].get() }
	}
}

impl<'a, T> IndexMut<Entity> for LocatedStorage<'a, T> {
	fn index_mut(&mut self, loc: Entity) -> &mut Self::Output {
		unsafe { &mut *self.storage[loc].get() }
	}
}

impl<'a, T> StorageView for LocatedStorage<'a, T> {
	type Comp = T;

	fn get(&self, entity: Entity) -> Option<&Self::Comp> {
		// Name resolution prioritizes inherent method of the same name.
		self.get(entity)
	}

	fn has(&self, entity: Entity) -> bool {
		// Name resolution prioritizes inherent method of the same name.
		self.has(entity)
	}
}

impl<'a, T> StorageViewMut for LocatedStorage<'a, T> {
	fn get_mut(&mut self, entity: Entity) -> Option<&mut Self::Comp> {
		// Name resolution prioritizes inherent method of the same name.
		self.get_mut(entity)
	}
}

impl<'a, T> LocatedStorageView<'a> for LocatedStorage<'a, T> {
	type BackingComp = T;

	fn locate(&self, entity: Entity) -> CompLocation<'a, Self::BackingComp> {
		// Name resolution prioritizes inherent method of the same name.
		self.locate(entity)
	}
}

impl<'a, T> LocatedStorageViewMut<'a> for LocatedStorage<'a, T> {}

#[derive(Debug, Copy, Clone)]
pub struct LocatedStorageRun<'a, T> {
	key: u64,
	run: StorageRunView<'a, UnsafeCell<T>>,
}

impl<'a, T> LocatedStorageRun<'a, T> {
	pub fn run(&self) -> StorageRunView<'a, UnsafeCell<T>> {
		self.run
	}

	pub fn try_locate(&self, entity: Entity) -> Option<CompLocation<'a, T>> {
		self.run.try_get(entity).map(|value| CompLocation {
			value,
			entity,
			key: self.key,
		})
	}

	pub fn locate(&self, entity: Entity) -> CompLocation<'a, T> {
		CompLocation {
			value: &self.run.get(entity),
			entity,
			key: self.key,
		}
	}
}

#[derive(Debug)]
#[derive_where(Copy, Clone)]
pub struct CompLocation<'a, T> {
	value: &'a UnsafeCell<T>,
	entity: Entity,
	key: u64,
}

impl<'a, T> CompLocation<'a, T> {
	pub fn value(self) -> &'a UnsafeCell<T> {
		self.value
	}

	pub fn entity(self) -> Entity {
		self.entity
	}
}
