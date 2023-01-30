use std::{
	any::type_name,
	cell::UnsafeCell,
	fmt::Debug,
	mem,
	ops::{self, Index, IndexMut},
	sync::atomic::{AtomicU64, Ordering},
};

use derive_where::derive_where;

use crate::{
	debug::lifetime::{DebugLifetime, DebugLifetimeWrapper, Dependent},
	entity::hashers::ArchetypeBuildHasher,
	query::{QueryIter, StorageIterMut, StorageIterRef},
	util::{
		ptr::PointeeCastExt,
		transmute::{TransMap, TransVec},
	},
	ArchetypeId, Entity, Query,
};

// === StorageLike === //

pub trait StorageLike: ops::Index<Entity, Output = Self::Comp> {
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

pub trait StorageLikeMut: StorageLike + ops::IndexMut<Entity, Output = Self::Comp> {
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

#[derive(Debug)]
pub struct MappedStorageRef<'a, S: ?Sized, M> {
	pub storage: &'a S,
	pub mapper: M,
}

impl<'a, S, M> ops::Index<Entity> for MappedStorageRef<'a, S, M>
where
	S: ?Sized + StorageLike,
	M: RefMapper<S::Comp>,
{
	type Output = M::Out;

	fn index(&self, entity: Entity) -> &Self::Output {
		self.mapper.map_ref(&self.storage[entity])
	}
}

impl<'a, S, M> StorageLike for MappedStorageRef<'a, S, M>
where
	S: ?Sized + StorageLike,
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
	S: ?Sized + StorageLike,
	M: RefMapper<S::Comp>,
{
	type Output = M::Out;

	fn index(&self, entity: Entity) -> &Self::Output {
		self.mapper.map_ref(&self.storage[entity])
	}
}

impl<'a, S, M> ops::IndexMut<Entity> for MappedStorageMut<'a, S, M>
where
	S: ?Sized + StorageLikeMut,
	M: MutMapper<S::Comp>,
{
	fn index_mut(&mut self, entity: Entity) -> &mut Self::Output {
		self.mapper.map_mut(&mut self.storage[entity])
	}
}

impl<'a, S, M> StorageLike for MappedStorageMut<'a, S, M>
where
	S: ?Sized + StorageLike,
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

impl<'a, S, M> StorageLikeMut for MappedStorageMut<'a, S, M>
where
	S: ?Sized + StorageLikeMut,
	M: MutMapper<S::Comp>,
{
	fn get_mut(&mut self, entity: Entity) -> Option<&mut Self::Comp> {
		self.storage.get_mut(entity).map(|v| self.mapper.map_mut(v))
	}
}

// === Storage === //

fn failed_to_find_component<T>(entity: Entity) -> ! {
	panic!(
		"failed to find component of type {} for entity {entity:?}",
		type_name::<T>()
	);
}

#[derive(Debug, Clone)]
#[derive_where(Default)]
#[repr(C)]
pub struct Storage<T> {
	archetypes: TransMap<ArchetypeId, StorageRun<()>, StorageRun<T>, ArchetypeBuildHasher>,
}

impl<T> Storage<T> {
	pub fn new() -> Self {
		Self {
			archetypes: TransMap::default(),
		}
	}

	pub fn as_celled(&mut self) -> &mut Storage<UnsafeCell<T>> {
		unsafe { self.transmute_mut_via_ptr(|p| p.cast()) }
	}

	pub fn as_wrapped<'r, W: StorageWrapper<'r, Comp = T>>(&'r mut self) -> W {
		W::wrap(self)
	}

	pub fn get_run(&self, archetype: ArchetypeId) -> Option<&StorageRun<T>> {
		if archetype.is_condemned() {
			log::error!("Acquired the storage run of the dead archetype {archetype:?}.");
			// (fallthrough)
		}

		self.archetypes.get(&archetype)
	}

	pub fn get_run_mut(&mut self, archetype: ArchetypeId) -> Option<&mut StorageRun<T>> {
		if archetype.is_condemned() {
			log::error!("Acquired the storage run of the dead archetype {archetype:?}.");
			// (fallthrough)
		}

		self.archetypes.get_mut(&archetype)
	}

	pub fn get_run_view(&self, archetype: ArchetypeId) -> StorageRunView<T> {
		self.get_run(archetype).map_or(
			StorageRunView::new_empty(archetype),
			StorageRun::as_ref_view,
		)
	}

	pub fn get_run_slice(&self, archetype: ArchetypeId) -> &StorageSlotSlice<T> {
		self.get_run(archetype).map_or(&[], StorageRun::as_slice)
	}

	pub fn get_run_slice_mut(&mut self, archetype: ArchetypeId) -> &mut StorageSlotSlice<T> {
		self.get_run_mut(archetype)
			.map_or(&mut [], StorageRun::as_mut_slice)
	}

	pub fn get_or_create_run(&mut self, archetype: ArchetypeId) -> &mut StorageRun<T> {
		if archetype.is_condemned() {
			log::error!("Acquired the storage run of the dead archetype {archetype:?}");
			// (fallthrough)
		}

		self.archetypes
			.get_mut_or_create(archetype, || StorageRun::new(archetype))
	}

	pub fn insert(&mut self, entity: Entity, value: T) -> (Option<T>, &mut T) {
		self.get_or_create_run(entity.archetype) // warns on dead archetype
			.insert(entity, value) // warns on dead entity
	}

	pub fn add(&mut self, entity: Entity, value: T) -> &mut T {
		let run = self.get_or_create_run(entity.archetype);

		if cfg!(debug_assertions) && run.get_slot_by_idx(entity.slot).is_some() {
			log::warn!(
				"`.add`'ed a component of type {} to an entity {:?} that already had the component. \
			     Use `.insert` instead if you wish to replace pre-existing components silently.",
				type_name::<T>(),
				entity,
			);
			// (fallthrough)
		}

		run.insert(entity, value).1
	}

	pub fn try_remove(&mut self, entity: Entity) -> Option<T> {
		if entity.is_condemned() {
			log::error!(
				"Removed a component of type {} from the already-dead entity {:?}. \
				 Please remove all components from an entity *before* destroying them to avoid UAF bugs.",
				type_name::<T>(),
				entity,
			);
			// (fallthrough)
		}

		let run = self.archetypes.get_mut(&entity.archetype)?;
		let removed = run.remove(entity.slot);

		if removed.is_some() && run.as_slice().is_empty() {
			self.archetypes.remove(&entity.archetype);
		}

		removed
	}

	pub fn try_remove_many<I>(&mut self, entities: I)
	where
		I: IntoIterator<Item = Entity>,
	{
		for entity in entities {
			self.try_remove(entity);
		}
	}

	pub fn remove(&mut self, entity: Entity) {
		let res = self.try_remove(entity);
		if cfg!(debug_assertions) && res.is_none() {
			log::warn!(
				"Removed a component of type {} from entity {:?}, which didn't have that component. \
				 Use `.try_remove` instead if you wish to ignore removals from entities without the component.",
				type_name::<T>(),
				entity,
			);
			// (fallthrough)
		}
	}

	pub fn get(&self, entity: Entity) -> Option<&T> {
		if entity.is_condemned() {
			log::error!(
				"Fetched component of type {} from the dead entity {entity:?}.",
				type_name::<T>()
			);
			// (fallthrough)
		}

		self.archetypes
			.get(&entity.archetype)?
			.get_slot_by_idx(entity.slot)
			.map(|(_, value)| value)
	}

	pub fn get_mut(&mut self, entity: Entity) -> Option<&mut T> {
		if entity.is_condemned() {
			log::error!(
				"Fetched component of type {} from the dead entity {entity:?}.",
				type_name::<T>()
			);
			// (fallthrough)
		}

		self.archetypes
			.get_mut(&entity.archetype)?
			.get_slot_by_idx_mut(entity.slot)
			.map(|(_, v)| v)
	}

	pub fn has(&self, entity: Entity) -> bool {
		self.get(entity).is_some()
	}

	pub fn clear(&mut self) {
		self.archetypes.clear();
	}

	pub fn query_in_ref(&self, archetype: ArchetypeId) -> QueryIter<(StorageIterRef<T>,)> {
		(self,).query_in(archetype)
	}

	pub fn query_in_mut(&mut self, archetype: ArchetypeId) -> QueryIter<(StorageIterMut<T>,)> {
		(self,).query_in(archetype)
	}
}

impl<T> ops::Index<Entity> for Storage<T> {
	type Output = T;

	fn index(&self, entity: Entity) -> &Self::Output {
		self.get(entity)
			.unwrap_or_else(|| failed_to_find_component::<T>(entity))
	}
}

impl<T> ops::IndexMut<Entity> for Storage<T> {
	fn index_mut(&mut self, entity: Entity) -> &mut Self::Output {
		self.get_mut(entity)
			.unwrap_or_else(|| failed_to_find_component::<T>(entity))
	}
}

impl<T> StorageLike for Storage<T> {
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

impl<T> StorageLikeMut for Storage<T> {
	fn get_mut(&mut self, entity: Entity) -> Option<&mut Self::Comp> {
		// Name resolution prioritizes inherent method of the same name.
		self.get_mut(entity)
	}
}

// === StorageRun === //

#[derive(Debug)]
#[derive_where(Copy, Clone)]
pub struct StorageRunView<'a, T> {
	archetype: ArchetypeId,
	comps: &'a StorageSlotSlice<T>,
}

impl<'a, T> StorageRunView<'a, T> {
	// Constructors and getters
	pub fn new(archetype: ArchetypeId, slots: &'a StorageSlotSlice<T>) -> Self {
		Self {
			archetype,
			comps: slots,
		}
	}

	pub fn new_empty(archetype: ArchetypeId) -> Self {
		Self::new(archetype, &[])
	}

	pub fn archetype(self) -> ArchetypeId {
		self.archetype
	}

	pub fn as_slice(self) -> &'a StorageSlotSlice<T> {
		self.comps
	}

	// Getters
	pub fn get_slot_by_idx(self, slot_idx: u32) -> Option<(DebugLifetime, &'a T)> {
		let slot = self
			.comps
			.get(slot_idx as usize)
			.and_then(|slot| slot.pair());

		if let Some((lt, _)) = slot.filter(|(lt, _)| lt.is_condemned()) {
			log::error!(
				"Fetched a storage slot at index {} of type {:?} for the dead entity {:?}",
				slot_idx,
				type_name::<T>(),
				lt,
			);
			// (fallthrough)
		}

		slot
	}

	pub fn get_slot(self, entity: Entity) -> Option<(DebugLifetime, &'a T)> {
		// Validate handle
		if cfg!(debug_assertions) && entity.archetype != self.archetype {
			log::error!(
				"Attempted to get an entity from a different archetype {:?} into a storage run \
				 for entities of archetype {:?}",
				entity.archetype,
				self.archetype,
			);
			// (fallthrough)
		}

		if entity.is_condemned() {
			log::error!(
				"Attempted to get a component of type {:?} from the dead entity {entity:?}",
				type_name::<T>()
			);
			// (fallthrough)
		}

		// Get component
		self.get_slot_by_idx(entity.slot)
	}

	pub fn try_get(self, entity: Entity) -> Option<&'a T> {
		self.get_slot(entity).map(|(_, v)| v)
	}

	pub fn get(self, entity: Entity) -> &'a T {
		self.try_get(entity)
			.unwrap_or_else(|| failed_to_find_component::<T>(entity))
	}

	pub fn has_by_idx(self, slot_idx: u32) -> bool {
		self.get_slot_by_idx(slot_idx).is_some()
	}

	pub fn has(self, entity: Entity) -> bool {
		self.try_get(entity).is_some()
	}

	pub fn max_slot(self) -> u32 {
		self.comps.len() as u32
	}
}

#[derive(Debug, Clone)]
#[repr(C)]
pub struct StorageRun<T> {
	archetype: ArchetypeId,
	comps: TransVec<StorageSlot<T>>,
}

impl<T> StorageRun<T> {
	// Constructors and getters
	pub fn new(archetype: ArchetypeId) -> Self {
		Self {
			archetype,
			comps: TransVec::new(),
		}
	}

	pub fn archetype(&self) -> ArchetypeId {
		self.archetype
	}

	pub fn as_slice(&self) -> &StorageSlotSlice<T> {
		self.comps.get_slice()
	}

	pub fn as_mut_slice(&mut self) -> &mut StorageSlotSlice<T> {
		self.comps.get_mut_slice()
	}

	pub fn as_ref_view(&self) -> StorageRunView<'_, T> {
		StorageRunView::new(self.archetype, self.as_slice())
	}

	// Special manipulation methods
	// TODO: Implement auto-deletion of empty runs and expose these.

	fn insert(&mut self, entity: Entity, value: T) -> (Option<T>, &mut T) {
		// Validate handles
		if cfg!(debug_assertions) && entity.archetype != self.archetype {
			log::error!(
				"Attempted to insert an entity from a different archetype {:?} into a storage run \
				 for entities of archetype {:?}",
				entity.archetype,
				self.archetype,
			);
			// (fallthrough)
		}

		if entity.is_condemned() {
			log::error!(
				"Attempted to attach a component of type {:?} to the dead entity {entity:?}",
				type_name::<T>()
			);
			// (fallthrough)
		}

		// Get slot
		let slot_idx = entity.slot_usize();
		if slot_idx >= self.comps.get_slice().len() {
			self.comps
				.mutate(|comps| comps.resize_with(slot_idx + 1, || StorageSlot::Empty));
		};

		let slot = &mut self.comps.get_mut_slice()[slot_idx];

		// Replace slot
		let replaced = mem::replace(
			slot,
			StorageSlot::Full {
				lifetime: Dependent::new(entity.lifetime),
				value,
			},
		);

		(replaced.into_value(), slot.value_mut().unwrap())
	}

	fn remove(&mut self, slot: u32) -> Option<T> {
		self.comps.mutate(|comps| {
			let removed = mem::replace(comps.get_mut(slot as usize)?, StorageSlot::Empty);

			while matches!(comps.last(), Some(StorageSlot::Empty)) {
				comps.pop();
			}

			removed.into_value()
		})
	}

	// Forwarded accessors
	pub fn get_slot_by_idx(&self, slot_idx: u32) -> Option<(DebugLifetime, &T)> {
		self.as_ref_view().get_slot_by_idx(slot_idx)
	}

	pub fn get_slot(&self, entity: Entity) -> Option<(DebugLifetime, &T)> {
		self.as_ref_view().get_slot(entity)
	}

	pub fn get(&self, entity: Entity) -> Option<&T> {
		self.as_ref_view().try_get(entity)
	}

	pub fn has_by_idx(&self, slot_idx: u32) -> bool {
		self.as_ref_view().has_by_idx(slot_idx)
	}

	pub fn has(&self, entity: Entity) -> bool {
		self.as_ref_view().has(entity)
	}

	pub fn max_slot(&self) -> u32 {
		self.as_ref_view().max_slot()
	}

	// Mutable accessors
	pub fn get_slot_by_idx_mut(&mut self, slot_idx: u32) -> Option<(DebugLifetime, &mut T)> {
		let slot = self
			.comps
			.get_mut_slice()
			.get_mut(slot_idx as usize)
			.and_then(|slot| slot.pair_mut());

		if let Some((lt, _)) = slot.as_ref().filter(|(lt, _)| lt.is_condemned()) {
			log::error!(
				"Fetched a storage slot at index {} of type {:?} for the dead entity {:?}",
				slot_idx,
				type_name::<T>(),
				lt,
			);
			// (fallthrough)
		}

		slot
	}

	pub fn get_slot_mut(&mut self, entity: Entity) -> Option<(DebugLifetime, &mut T)> {
		// Validate handle
		if cfg!(debug_assertions) && entity.archetype != self.archetype {
			log::error!(
				"Attempted to get an entity from a different archetype {:?} into a storage run \
				 for entities of archetype {:?}",
				entity.archetype,
				self.archetype,
			);
			// (fallthrough)
		}

		if entity.is_condemned() {
			log::error!(
				"Attempted to get a component of type {:?} from the dead entity {entity:?}",
				type_name::<T>()
			);
			// (fallthrough)
		}

		// Get component
		self.get_slot_by_idx_mut(entity.slot)
	}

	pub fn get_mut(&mut self, entity: Entity) -> Option<&mut T> {
		self.get_slot_mut(entity).map(|(_, v)| v)
	}
}

impl<T> ops::Index<Entity> for StorageRun<T> {
	type Output = T;

	fn index(&self, entity: Entity) -> &Self::Output {
		self.get(entity)
			.unwrap_or_else(|| failed_to_find_component::<T>(entity))
	}
}

impl<T> ops::IndexMut<Entity> for StorageRun<T> {
	fn index_mut(&mut self, entity: Entity) -> &mut Self::Output {
		self.get_mut(entity)
			.unwrap_or_else(|| failed_to_find_component::<T>(entity))
	}
}

impl<T> StorageLike for StorageRun<T> {
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

impl<T> StorageLikeMut for StorageRun<T> {
	fn get_mut(&mut self, entity: Entity) -> Option<&mut Self::Comp> {
		// Name resolution prioritizes inherent method of the same name.
		self.get_mut(entity)
	}
}

// === StorageRunSlot === //

pub type StorageSlotSlice<T> = [StorageSlot<T>];

// This actually has a defined representation.
// See: https://doc.rust-lang.org/reference/type-layout.html#reprc-enums-with-fields
#[derive(Debug, Clone)]
#[derive_where(Default)]
#[repr(C)]
pub enum StorageSlot<T> {
	Full {
		lifetime: Dependent<DebugLifetime>,
		value: T,
	},
	#[derive_where(default)]
	Empty,
}

impl<T> StorageSlot<T> {
	pub fn is_full(&self) -> bool {
		self.value().is_some()
	}

	pub fn into_pair(self) -> Option<(DebugLifetime, T)> {
		match self {
			StorageSlot::Full { value, lifetime } => Some((lifetime.get(), value)),
			StorageSlot::Empty => None,
		}
	}

	pub fn pair(&self) -> Option<(DebugLifetime, &T)> {
		match self {
			StorageSlot::Full { value, lifetime } => Some((lifetime.get(), value)),
			StorageSlot::Empty => None,
		}
	}

	pub fn pair_mut(&mut self) -> Option<(DebugLifetime, &mut T)> {
		match self {
			StorageSlot::Full { value, lifetime } => Some((lifetime.get(), value)),
			StorageSlot::Empty => None,
		}
	}

	pub fn into_value(self) -> Option<T> {
		match self {
			StorageSlot::Full { value, .. } => Some(value),
			StorageSlot::Empty => None,
		}
	}

	pub fn value(&self) -> Option<&T> {
		match self {
			StorageSlot::Full { value, .. } => Some(value),
			StorageSlot::Empty => None,
		}
	}

	pub fn value_mut(&mut self) -> Option<&mut T> {
		match self {
			StorageSlot::Full { value, .. } => Some(value),
			StorageSlot::Empty => None,
		}
	}
}

// === Wrappers === //

pub trait StorageWrapper<'r> {
	type Comp;

	fn wrap(storage: &'r mut Storage<Self::Comp>) -> Self;
}

#[derive(Debug)]
pub struct LocatedStorage<'a, T> {
	storage: &'a Storage<UnsafeCell<T>>,
	key: u64,
}

// TODO: Implement `LocatedStorageRun` for even more performance gains.

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

impl<'a, T> StorageLike for LocatedStorage<'a, T> {
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

impl<'a, T> StorageLikeMut for LocatedStorage<'a, T> {
	fn get_mut(&mut self, entity: Entity) -> Option<&mut Self::Comp> {
		// Name resolution prioritizes inherent method of the same name.
		self.get_mut(entity)
	}
}

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
