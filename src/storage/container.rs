use std::{any::type_name, cell::UnsafeCell, mem, ops};

use derive_where::derive_where;

use crate::{
	debug::lifetime::{DebugLifetime, DebugLifetimeWrapper},
	entity::hashers::ArchetypeBuildHasher,
	util::{
		ptr::PointeeCastExt,
		transmute::{TransMap, TransVec},
	},
	ArchetypeId, Dependent, Entity, Query, StorageView, StorageViewMut,
};

use super::{
	query::{QueryIter, StorageIterMut, StorageIterRef},
	wrapper::StorageWrapper,
};

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

impl<T> StorageView for Storage<T> {
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

impl<T> StorageViewMut for Storage<T> {
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

impl<T> StorageView for StorageRun<T> {
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

impl<T> StorageViewMut for StorageRun<T> {
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
