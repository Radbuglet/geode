use std::{any::type_name, collections::HashMap, fmt::Debug, num::NonZeroU32, ops};

use derive_where::derive_where;

use crate::{debug::lifetime::{DebugLifetime, Dependent, LifetimeLike}, mem::no_hash::NoOpBuildHasher};

use super::{
	entity::{ArchetypeId, Entity},
	query::{Query, QueryIter, StorageIterMut, StorageIterRef},
	universe::{BuildableResourceRw, Universe},
};

fn failed_to_find_component<T>(entity: Entity) -> ! {
	panic!(
		"failed to find entity {entity:?} with component {}",
		type_name::<T>()
	);
}

#[derive(Debug, Clone)]
#[derive_where(Default)]
#[repr(transparent)]
pub struct Storage<T> {
	archetypes: HashMap<NonZeroU32, StorageRun<T>, NoOpBuildHasher>,
}

impl<T> Storage<T> {
	pub fn new() -> Self {
		Self {
			archetypes: HashMap::default(),
		}
	}

	pub fn get_run(&self, archetype: ArchetypeId) -> Option<&StorageRun<T>> {
		if archetype.is_condemned() {
			log::error!("Acquired the storage run of the dead archetype {archetype:?}.");
			// (fallthrough)
		}

		self.archetypes.get(&archetype.id)
	}

	pub fn get_run_mut(&mut self, archetype: ArchetypeId) -> Option<&mut StorageRun<T>> {
		if archetype.is_condemned() {
			log::error!("Acquired the storage run of the dead archetype {archetype:?}.");
			// (fallthrough)
		}

		self.archetypes.get_mut(&archetype.id)
	}

	pub fn get_run_slice(&self, archetype: ArchetypeId) -> &[Option<StorageRunSlot<T>>] {
		match self.get_run(archetype) {
			Some(run) => run.as_slice(),
			None => &[],
		}
	}

	pub fn get_run_slice_mut(
		&mut self,
		archetype: ArchetypeId,
	) -> &mut [Option<StorageRunSlot<T>>] {
		match self.get_run_mut(archetype) {
			Some(run) => run.as_mut_slice(),
			None => &mut [],
		}
	}

	pub fn get_or_create_run(&mut self, archetype: ArchetypeId) -> &mut StorageRun<T> {
		if archetype.is_condemned() {
			log::error!("Acquired the storage run of the dead archetype {archetype:?}");
			// (fallthrough)
		}

		self.archetypes
			.entry(archetype.id)
			.or_insert_with(Default::default)
	}

	pub fn insert(&mut self, entity: Entity, value: T) -> (Option<T>, &mut T) {
		self.get_or_create_run(entity.arch) // warns on dead archetype
			.insert(entity, value) // warns on dead entity
	}

	pub fn add(&mut self, entity: Entity, value: T) -> &mut T {
		let run = self.get_or_create_run(entity.arch);

		if cfg!(debug_assertions) && run.get(entity.slot).is_some() {
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

		let run = self.archetypes.get_mut(&entity.arch.id)?;
		let removed = run.remove(entity.slot);

		if removed.is_some() && run.as_slice().is_empty() {
			self.archetypes.remove(&entity.arch.id);
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
			.get(&entity.arch.id)?
			.get(entity.slot)
			.map(StorageRunSlot::value)
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
			.get_mut(&entity.arch.id)?
			.get_mut(entity.slot)
			.map(StorageRunSlot::value_mut)
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

pub type StorageRunSlice<T> = [Option<StorageRunSlot<T>>];

#[derive(Debug, Clone)]
#[derive_where(Default)]
pub struct StorageRun<T> {
	comps: Vec<Option<StorageRunSlot<T>>>,
}

impl<T> StorageRun<T> {
	pub fn new() -> Self {
		Self { comps: Vec::new() }
	}

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
		if !(slot_idx < self.comps.len()) {
			self.comps.resize_with(slot_idx + 1, || None);
		};
		let slot = &mut self.comps[slot_idx];

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
		let removed = self.comps.get_mut(slot as usize)?.take().map(|v| v.value);

		while matches!(self.comps.last(), Some(None)) {
			self.comps.pop();
		}

		removed
	}

	pub fn get(&self, slot_idx: u32) -> Option<&StorageRunSlot<T>> {
		let slot = self
			.comps
			.get(slot_idx as usize)
			.and_then(|opt| opt.as_ref());

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
			.comps
			.get_mut(slot_idx as usize)
			.and_then(|opt| opt.as_mut());

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

	pub fn max_slot(&self) -> u32 {
		self.comps.len() as u32
	}

	pub fn as_slice(&self) -> &StorageRunSlice<T> {
		self.comps.as_slice()
	}

	pub fn as_mut_slice(&mut self) -> &mut StorageRunSlice<T> {
		self.comps.as_mut_slice()
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

impl<T: 'static + Send + Sync> BuildableResourceRw for Storage<T> {
	fn create(_universe: &Universe) -> Self {
		Default::default()
	}
}
