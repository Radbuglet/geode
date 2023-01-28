use std::{any::type_name, collections::HashMap, marker::PhantomData, mem, ops, ptr::NonNull};

use parking_lot::Mutex;

use crate::{
	debug::lifetime::{DebugLifetime, DebugLifetimeWrapper},
	entity::hashers,
	ArchetypeId, Dependent, Entity,
};

// === Traits === //

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

// === Storage === //

fn failed_to_find_component<T>(entity: Entity) -> ! {
	panic!(
		"failed to find entity {entity:?} with component {}",
		type_name::<T>()
	);
}

pub struct ShardedStorage<T: 'static> {
	inner: Mutex<UnshardedStorage<T>>,
}

pub struct UnshardedStorage<T: 'static> {
	storage: Storage<'static, T>,
}

pub struct StorageShard<'p, T: 'static> {
	storage: Storage<'p, T>,
}

pub struct Storage<'p, T: 'static> {
	shard_owner: Option<&'p ShardedStorage<T>>,
	runs: HashMap<ArchetypeId, StorageRun<'p, T>, hashers::ArchetypeBuildHasher>,
}

// === Storage Run === //

pub struct StorageRun<'p, T: 'static> {
	_owner_lt: PhantomData<&'p [Option<StorageRunSlot<T>>]>,
	// If `cap == usize::MAX`, this is an immutable reference to a slice
	// owned by a referent with lifetime `'p`. Otherwise, this is the real
	// capacity of a vector we own.
	cap: usize,
	slice: NonNull<[Option<StorageRunSlot<T>>]>,
}

impl<T: 'static> StorageRun<'static, T> {
	pub fn new() -> Self {
		Self {
			_owner_lt: PhantomData,
			cap: 0,
			slice: NonNull::from(Vec::new().leak()),
		}
	}
}

impl<'p, T: 'static> StorageRun<'p, T> {
	const OWNED_ACCESS_OF_REF_ERR: &str =
		"attempted to access an immutable storage run as an owned instance";

	pub fn is_owned(&self) -> bool {
		self.cap != usize::MAX
	}

	pub fn as_slice(&self) -> &[Option<StorageRunSlot<T>>] {
		unsafe { self.slice.as_ref() }
	}

	pub fn try_as_mut_slice(&mut self) -> Option<&mut [Option<StorageRunSlot<T>>]> {
		if self.is_owned() {
			Some(unsafe { self.slice.as_mut() })
		} else {
			None
		}
	}

	pub fn as_mut_slice(&mut self) -> &mut [Option<StorageRunSlot<T>>] {
		self.try_as_mut_slice()
			.expect(Self::OWNED_ACCESS_OF_REF_ERR)
	}

	unsafe fn slot_vec(&mut self) -> Option<Vec<Option<StorageRunSlot<T>>>> {
		if self.is_owned() {
			let ptr = self.slice.as_ptr().cast::<Option<StorageRunSlot<T>>>();
			let len = self.slice.len();
			let cap = self.cap;

			Some(Vec::from_raw_parts(ptr, len, cap))
		} else {
			None
		}
	}

	pub fn update_slot_vec<F, R>(&mut self, func: F) -> R
	where
		F: FnOnce(&mut Vec<Option<StorageRunSlot<T>>>) -> R,
	{
		struct DropGuard<'a, 'p, T: 'static> {
			run: &'a mut StorageRun<'p, T>,
			vec: mem::ManuallyDrop<Vec<Option<StorageRunSlot<T>>>>,
		}

		impl<T: 'static> Drop for DropGuard<'_, '_, T> {
			fn drop(&mut self) {
				self.run.cap = self.vec.capacity();
				self.run.slice = NonNull::from(self.vec.as_mut_slice());
			}
		}

		let vec = unsafe { self.slot_vec() }.expect(Self::OWNED_ACCESS_OF_REF_ERR);
		let mut guard = DropGuard {
			run: self,
			vec: mem::ManuallyDrop::new(vec),
		};

		func(&mut guard.vec)
	}
}

impl<'p, T: 'static> StorageRun<'p, T> {
	fn insert(&mut self, entity: Entity, value: T) -> (Option<T>, &mut T) {
		if entity.is_condemned() {
			log::error!(
				"Attempted to attach a component of type {:?} to the dead entity {entity:?}",
				type_name::<T>()
			);
			// (fallthrough)
		}

		// Get slot
		let slot_idx = entity.slot_usize();
		if slot_idx >= self.as_slice().len() {
			self.update_slot_vec(|slots| slots.resize_with(slot_idx + 1, || None));
		};

		let slot = &mut self.as_mut_slice()[slot_idx];

		// Replace slot
		let replaced = slot
			.replace(StorageRunSlot {
				lifetime: Dependent::new(entity.lifetime),
				value,
			})
			.map(|v| v.value);

		(replaced, slot.as_mut().unwrap().value_mut())
	}

	fn remove(&mut self, slot: u32) -> Option<T> {
		self.update_slot_vec(|slots| {
			let removed = slots.get_mut(slot as usize)?.take().map(|v| v.value);

			while matches!(slots.last(), Some(None)) {
				slots.pop();
			}

			removed
		})
	}

	pub fn get(&self, slot_idx: u32) -> Option<&StorageRunSlot<T>> {
		let slot = self
			.as_slice()
			.get(slot_idx as usize)
			.and_then(Option::as_ref);

		if let Some(slot) = slot.filter(|slot| slot.lifetime.get().is_condemned()) {
			log::error!(
				"Fetched a storage slot at index {} of type {:?} for the dead entity {:?}",
				slot_idx,
				type_name::<T>(),
				slot.lifetime.get(),
			);
			// (fallthrough)
		}

		slot
	}

	pub fn get_mut(&mut self, slot_idx: u32) -> Option<&mut StorageRunSlot<T>> {
		let slot = self
			.as_mut_slice()
			.get_mut(slot_idx as usize)
			.and_then(Option::as_mut);

		if let Some(slot) = slot
			.as_ref()
			.filter(|slot| slot.lifetime.get().is_condemned())
		{
			log::error!(
				"Fetched a storage slot at index {} of type {:?} for the dead entity {:?}",
				slot_idx,
				type_name::<T>(),
				slot.lifetime.get(),
			);
			// (fallthrough)
		}

		slot
	}

	pub fn is_empty(&self) -> bool {
		self.as_slice().is_empty()
	}

	pub fn max_slot(&self) -> u32 {
		self.as_slice().len() as u32
	}
}

impl<T: 'static> Drop for StorageRun<'_, T> {
	fn drop(&mut self) {
		drop(unsafe { self.slot_vec() });
	}
}

#[derive(Debug, Clone)]
pub struct StorageRunSlot<T> {
	lifetime: Dependent<DebugLifetime>,
	value: T,
}

impl<T> StorageRunSlot<T> {
	pub fn lifetime(&self) -> DebugLifetime {
		self.lifetime.get()
	}

	pub fn value(&self) -> &T {
		&self.value
	}

	pub fn value_mut(&mut self) -> &mut T {
		&mut self.value
	}
}
