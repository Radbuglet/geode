use derive_where::derive_where;
use std::{
	any::type_name,
	collections::{HashMap, HashSet},
	marker::PhantomData,
	mem::transmute,
	num::NonZeroU32,
	ops::{Index, IndexMut},
};

use parking_lot::Mutex;

use crate::{
	debug::{
		label::{DebugLabel, NO_LABEL},
		lifetime::{DebugLifetime, FloatingLifetimeLike, Lifetime, LifetimeLike, OwnedLifetime},
	},
	util::{free_list::FreeList, no_hash::RandIdGen},
	Bundle, Dependent, ExclusiveUniverse,
};

// === Handles === //

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct ArchetypeId {
	pub lifetime: DebugLifetime,
	pub id: NonZeroU32,
}

impl ArchetypeId {
	pub fn as_dependent(self) -> Dependent<Self> {
		FloatingLifetimeLike::as_dependent(self)
	}
}

impl FloatingLifetimeLike for ArchetypeId {
	fn is_possibly_alive(self) -> bool {
		self.lifetime.is_possibly_alive()
	}

	fn is_condemned(self) -> bool {
		self.lifetime.is_condemned()
	}

	fn inc_dep(self) {
		self.lifetime.inc_dep();
	}

	fn dec_dep(self) {
		self.lifetime.dec_dep();
	}
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub struct WeakArchetypeId {
	pub lifetime: Lifetime,
	pub id: NonZeroU32,
}

impl WeakArchetypeId {
	pub fn as_regular(self) -> ArchetypeId {
		ArchetypeId {
			lifetime: self.lifetime.into(),
			id: self.id,
		}
	}

	pub fn as_dependent(self) -> Dependent<Self> {
		FloatingLifetimeLike::as_dependent(self)
	}

	pub fn is_alive(self) -> bool {
		self.lifetime.is_alive()
	}
}

impl LifetimeLike for WeakArchetypeId {
	fn is_alive(self) -> bool {
		// Name resolution prioritizes inherent method of the same name.
		self.is_alive()
	}
}

impl FloatingLifetimeLike for WeakArchetypeId {
	fn is_possibly_alive(self) -> bool {
		self.lifetime.is_possibly_alive()
	}

	fn is_condemned(self) -> bool {
		self.lifetime.is_condemned()
	}

	fn inc_dep(self) {
		self.lifetime.inc_dep();
	}

	fn dec_dep(self) {
		self.lifetime.dec_dep();
	}
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct Entity {
	pub lifetime: DebugLifetime,
	pub arch: ArchetypeId,
	pub slot: u32,
}

impl Entity {
	pub fn as_dependent(self) -> Dependent<Self> {
		FloatingLifetimeLike::as_dependent(self)
	}

	pub fn slot_usize(&self) -> usize {
		self.slot as usize
	}
}

impl FloatingLifetimeLike for Entity {
	fn is_possibly_alive(self) -> bool {
		self.lifetime.is_possibly_alive()
	}

	fn is_condemned(self) -> bool {
		self.lifetime.is_condemned()
	}

	fn inc_dep(self) {
		self.lifetime.inc_dep();
	}

	fn dec_dep(self) {
		self.lifetime.dec_dep();
	}
}

// === ID allocation === //

static ID_FREE_LIST: Mutex<Option<RandIdGen>> = Mutex::new(None);

fn alloc_id() -> NonZeroU32 {
	ID_FREE_LIST
		.lock()
		.get_or_insert_with(Default::default)
		.alloc()
}

fn dealloc_id(id: NonZeroU32) {
	ID_FREE_LIST
		.lock()
		.get_or_insert_with(Default::default)
		.dealloc(id);
}

// === Archetype === //

#[derive_where(Debug)]
#[repr(C)]
pub struct Archetype<M: ?Sized = ()> {
	_ty: PhantomData<fn(M) -> M>,
	id: NonZeroU32,
	lifetime: OwnedLifetime<Lifetime>,
	slots: FreeList<OwnedLifetime<DebugLifetime>>,
}

impl<M: ?Sized> Archetype<M> {
	pub fn new<L: DebugLabel>(name: L) -> Self {
		Self {
			_ty: PhantomData,
			id: alloc_id(),
			lifetime: OwnedLifetime::new(Lifetime::new(name)),
			slots: FreeList::default(),
		}
	}

	pub fn spawn<L: DebugLabel>(&mut self, name: L) -> Entity {
		let lifetime = DebugLifetime::new(name);
		let slot = self.slots.alloc(lifetime.into());

		// Construct handle
		Entity {
			lifetime,
			arch: self.id(),
			slot,
		}
	}

	pub fn spawn_with<L: DebugLabel>(&mut self, cx: M::Context<'_>, name: L, bundle: M) -> Entity
	where
		M: Bundle,
	{
		let target = self.spawn(name);
		bundle.attach(cx, target);
		target
	}

	pub fn spawn_with_auto_cx<L: DebugLabel>(
		&mut self,
		cx: &mut ExclusiveUniverse,
		name: L,
		bundle: M,
	) -> Entity
	where
		M: Bundle,
	{
		let target = self.spawn(name);
		bundle.attach_auto_cx(cx, target);
		target
	}

	pub fn despawn(&mut self, entity: Entity) {
		if cfg!(debug_assertions) && entity.arch.id != self.id {
			log::error!(
				"Attempted to despawn {:?} from the non-owning archetype {:?}.",
				entity,
				self
			);
			return;
		}

		if entity.lifetime.is_condemned() {
			log::error!(
				"Attempted to despawn the dead entity {:?} from the archetype {:?}",
				entity,
				self
			);
			return;
		}

		self.slots.dealloc(entity.slot);
	}

	pub fn despawn_and_extract(&mut self, cx: M::Context<'_>, entity: Entity) -> M
	where
		M: Bundle,
	{
		let bundle = M::detach(cx, entity);
		self.despawn(entity);
		bundle
	}

	pub fn despawn_and_extract_auto_cx(&mut self, cx: &mut ExclusiveUniverse, entity: Entity) -> M
	where
		M: Bundle,
	{
		let bundle = M::detach_auto_cx(cx, entity);
		self.despawn(entity);
		bundle
	}

	pub fn id(&self) -> ArchetypeId {
		ArchetypeId {
			lifetime: self.lifetime.get().into(),
			id: self.id,
		}
	}

	pub fn weak_id(&self) -> WeakArchetypeId {
		WeakArchetypeId {
			lifetime: self.lifetime.get(),
			id: self.id,
		}
	}

	pub fn lifetime(&self) -> Lifetime {
		self.lifetime.get()
	}

	pub fn cast_marker<N: ?Sized>(self) -> Archetype<N> {
		unsafe {
			// Safety: This struct is `repr(C)` and `N` is only ever used in a `PhantomData`.
			transmute(self)
		}
	}

	pub fn cast_marker_ref<N: ?Sized>(&self) -> &Archetype<N> {
		unsafe {
			// Safety: This struct is `repr(C)` and `N` is only ever used in a `PhantomData`.
			transmute(self)
		}
	}

	pub fn cast_marker_mut<N: ?Sized>(&mut self) -> &mut Archetype<N> {
		unsafe {
			// Safety: This struct is `repr(C)` and `N` is only ever used in a `PhantomData`.
			transmute(self)
		}
	}
}

impl<M: ?Sized> Default for Archetype<M> {
	fn default() -> Self {
		Self::new(NO_LABEL)
	}
}

impl<M: ?Sized> Drop for Archetype<M> {
	fn drop(&mut self) {
		dealloc_id(self.id);
	}
}

// === Maps === //

pub mod hashers {
	pub use crate::util::no_hash::NoOpBuildHasher as ArchetypeBuildHasher;
	pub use fnv::FnvBuildHasher as EntityBuildHasher;
}

pub type ArchetypeMap<V> = HashMap<Dependent<ArchetypeId>, V, hashers::ArchetypeBuildHasher>;
pub type ArchetypeSet = HashSet<Dependent<ArchetypeId>, hashers::ArchetypeBuildHasher>;
pub type EntityMap<V> = HashMap<Dependent<ArchetypeId>, V, hashers::EntityBuildHasher>;
pub type EntitySet = HashSet<Dependent<ArchetypeId>, hashers::EntityBuildHasher>;

// === Weak Maps === //

#[derive(Debug, Clone)]
#[derive_where(Default)]
pub struct WeakArchetypeMap<T> {
	map: HashMap<NonZeroU32, (Lifetime, T), hashers::ArchetypeBuildHasher>,
}

impl<T> WeakArchetypeMap<T> {
	pub fn new() -> Self {
		Self::default()
	}

	pub fn add(&mut self, id: WeakArchetypeId, value: T) -> Option<T> {
		let old = self.insert(id, value);

		if cfg!(debug_assertions) && old.is_some() {
			log::warn!(
				"`.add`'ed a component of type {} to an archetype {:?} that already had the component. \
			     Use `.insert` instead if you wish to replace pre-existing components silently.",
				type_name::<T>(),
				id
			);
			// (fallthrough)
		}

		old
	}

	pub fn insert(&mut self, id: WeakArchetypeId, value: T) -> Option<T> {
		// Ensure that this is the latest lifetime in its respective slot.
		if !id.lifetime.is_alive() {
			return None;
		}

		// Ensure that we won't grow the map if we insert a new entry by garbage
		// collecting where necessary.
		if self.map.len() >= self.map.capacity() {
			let old_len = self.map.len();
			self.gc();

			if self.map.len() == old_len {
				self.map.reserve(1);
			}
		}

		// Otherwise, just do the insertion normally.
		self.map
			.insert(id.id, (id.lifetime, value))
			.and_then(Self::filter_old_entries(id.lifetime))
	}

	pub fn try_remove(&mut self, id: WeakArchetypeId) -> Option<T> {
		// Dead archetypes technically map to none.
		if !id.lifetime.is_alive() {
			return None;
		}

		self.map
			.remove(&id.id)
			.and_then(Self::filter_old_entries(id.lifetime))
	}

	pub fn get(&self, id: WeakArchetypeId) -> Option<&T> {
		if !id.lifetime.is_alive() {
			return None;
		}

		self.map.get(&id.id).and_then(|(lt, value)| {
			if *lt == id.lifetime {
				Some(value)
			} else {
				None
			}
		})
	}

	pub fn get_mut(&mut self, id: WeakArchetypeId) -> Option<&mut T> {
		if !id.lifetime.is_alive() {
			return None;
		}

		self.map.get_mut(&id.id).and_then(|(lt, value)| {
			if *lt == id.lifetime {
				Some(value)
			} else {
				None
			}
		})
	}

	pub fn has(&self, id: WeakArchetypeId) -> bool {
		self.get(id).is_some()
	}

	fn filter_old_entries(latest: Lifetime) -> impl FnOnce((Lifetime, T)) -> Option<T> {
		move |(old_lt, value)| {
			// Filter out old values.
			if latest == old_lt {
				Some(value)
			} else {
				None
			}
		}
	}

	pub fn iter(&self) -> impl Iterator<Item = (WeakArchetypeId, &T)> + '_ {
		self.map.iter().filter_map(|(id, (lifetime, value))| {
			if lifetime.is_alive() {
				Some((
					WeakArchetypeId {
						lifetime: *lifetime,
						id: *id,
					},
					value,
				))
			} else {
				None
			}
		})
	}

	pub fn iter_mut(&mut self) -> impl Iterator<Item = (WeakArchetypeId, &mut T)> + '_ {
		self.gc();
		self.map.iter_mut().map(|(id, (lifetime, value))| {
			(
				WeakArchetypeId {
					lifetime: *lifetime,
					id: *id,
				},
				value,
			)
		})
	}

	pub fn keys(&self) -> impl Iterator<Item = WeakArchetypeId> + '_ {
		self.iter().map(|(k, _)| k)
	}

	pub fn values(&self) -> impl Iterator<Item = &T> + '_ {
		self.iter().map(|(_, v)| v)
	}

	pub fn values_mut(&mut self) -> impl Iterator<Item = &mut T> + '_ {
		self.iter_mut().map(|(_, v)| v)
	}

	pub fn clear(&mut self) {
		self.map.clear();
	}

	pub fn gc(&mut self) {
		self.map.retain(|_, (lt, _)| lt.is_alive())
	}
}

impl<T> Index<WeakArchetypeId> for WeakArchetypeMap<T> {
	type Output = T;

	fn index(&self, id: WeakArchetypeId) -> &Self::Output {
		self.get(id).unwrap()
	}
}

impl<T> IndexMut<WeakArchetypeId> for WeakArchetypeMap<T> {
	fn index_mut(&mut self, id: WeakArchetypeId) -> &mut Self::Output {
		self.get_mut(id).unwrap()
	}
}

// === Tests === //

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn ids_are_unique() {
		// TODO: Trap on erroneous logs.

		let mut arch_1 = Archetype::<()>::new("Archetype 1");
		let mut arch_2 = Archetype::<()>::new("Archetype 2");

		assert_ne!(arch_1.id(), arch_2.id());

		let entity_1 = arch_1.spawn("Entity 1");
		let entity_2 = arch_1.spawn("Entity 2");
		let entity_3 = arch_2.spawn("Entity 3");
		let entity_4 = arch_2.spawn("Entity 4");

		assert_eq!(entity_1.slot, 0);
		assert_eq!(entity_2.slot, 1);
		assert_eq!(entity_3.slot, 0);
		assert_eq!(entity_4.slot, 1);

		arch_1.despawn(entity_1);

		let entity_5 = arch_1.spawn("Entity 5");
		assert_eq!(entity_5.slot, 0);

		let entity_6 = arch_1.spawn("Entity 6");
		assert_eq!(entity_6.slot, 2);

		assert!(!entity_1.lifetime.raw().unwrap().is_alive());
		assert!(entity_2.lifetime.raw().unwrap().is_alive());
		assert!(entity_3.lifetime.raw().unwrap().is_alive());
		assert!(entity_4.lifetime.raw().unwrap().is_alive());
		assert!(entity_5.lifetime.raw().unwrap().is_alive());
		assert!(entity_6.lifetime.raw().unwrap().is_alive());

		arch_2.despawn(entity_3);
		assert!(!entity_3.lifetime.raw().unwrap().is_alive());

		let entity_7 = arch_2.spawn("Entity 7");
		assert_eq!(entity_7.slot, 0);
	}
}
