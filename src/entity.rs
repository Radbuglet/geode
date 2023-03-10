use derive_where::derive_where;
use std::{
	any::type_name,
	collections::{HashMap, HashSet},
	marker::PhantomData,
	mem::transmute,
	num::NonZeroU32,
	ops::{Index, IndexMut},
};

use parking_lot::{MappedRwLockReadGuard, MappedRwLockWriteGuard, Mutex, MutexGuard};

use crate::{
	debug::{
		label::{DebugLabel, NO_LABEL},
		lifetime::{DebugLifetime, DebugLifetimeWrapper, Lifetime, LifetimeWrapper, OwnedLifetime},
	},
	universe::BuildableArchetype,
	util::{free_list::FreeList, no_hash::RandIdGen},
	BypassExclusivity, Dependent, ExclusiveUniverse, Storage, StorageView, StorageViewMut,
	Universe,
};

// === Handles === //

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct ArchetypeId {
	pub lifetime: DebugLifetime,
	pub id: NonZeroU32,
}

impl ArchetypeId {
	pub fn get_in_universe(self, universe: &Universe) -> MutexGuard<Archetype> {
		universe.archetype_by_id(self)
	}
}

impl DebugLifetimeWrapper for ArchetypeId {
	fn as_debug_lifetime(me: Self) -> DebugLifetime {
		me.lifetime
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

	pub fn try_as_regular(self) -> Option<ArchetypeId> {
		self.filter_alive().map(Self::as_regular)
	}

	pub fn is_alive(self) -> bool {
		LifetimeWrapper::is_alive(self)
	}

	pub fn filter_alive(self) -> Option<Self> {
		LifetimeWrapper::filter_alive(self)
	}
}

impl LifetimeWrapper for WeakArchetypeId {
	fn as_lifetime(me: Self) -> Lifetime {
		me.lifetime
	}
}

impl DebugLifetimeWrapper for WeakArchetypeId {
	fn as_debug_lifetime(me: Self) -> DebugLifetime {
		me.lifetime.into()
	}
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct Entity {
	pub lifetime: DebugLifetime,
	pub archetype: ArchetypeId,
	pub slot: u32,
}

impl Entity {
	pub fn slot_usize(self) -> usize {
		self.slot as usize
	}

	pub fn comp<T>(self, storage: &impl StorageView<Comp = T>) -> &T {
		&storage[self]
	}

	pub fn comp_mut<T>(self, storage: &mut impl StorageViewMut<Comp = T>) -> &mut T {
		&mut storage[self]
	}

	pub fn comp_in_universe<T: 'static + Send + Sync>(
		self,
		universe: &Universe,
	) -> MappedRwLockReadGuard<T> {
		universe.comp(self)
	}

	pub fn comp_mut_in_universe<T: 'static + Send + Sync>(
		self,
		universe: &Universe,
	) -> MappedRwLockWriteGuard<T> {
		universe.comp_mut(self)
	}

	pub fn bypass_comp_in_universe<'r, T: 'static + Send + Sync + BypassExclusivity>(
		self,
		universe: &ExclusiveUniverse<'r>,
	) -> MappedRwLockReadGuard<'r, T> {
		universe.bypass_comp(self)
	}

	pub fn bypass_comp_mut_in_universe<'r, T: 'static + Send + Sync + BypassExclusivity>(
		self,
		universe: &ExclusiveUniverse<'r>,
	) -> MappedRwLockWriteGuard<'r, T> {
		universe.bypass_comp_mut(self)
	}
}

impl DebugLifetimeWrapper for Entity {
	fn as_debug_lifetime(me: Self) -> DebugLifetime {
		me.lifetime
	}
}

#[derive_where(Debug, Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct SingleEntity<T> {
	_ty: PhantomData<fn(T) -> T>,
	entity: Entity,
}

impl<T> SingleEntity<T> {
	pub fn new(entity: Entity) -> Self {
		Self {
			_ty: PhantomData,
			entity,
		}
	}

	pub fn as_entity(self) -> Entity {
		self.entity
	}

	pub fn cast<U>(self) -> SingleEntity<U> {
		SingleEntity::new(self.as_entity())
	}

	pub fn get<V: StorageView<Comp = T>>(self, storage: &V) -> &T {
		&storage[self.as_entity()]
	}

	pub fn get_mut<V: StorageViewMut<Comp = T>>(self, storage: &mut V) -> &mut T {
		&mut storage[self.as_entity()]
	}

	pub fn get_in_universe(self, universe: &Universe) -> MappedRwLockReadGuard<T>
	where
		T: 'static + Send + Sync,
	{
		universe.comp(self.as_entity())
	}

	pub fn get_mut_in_universe(self, universe: &Universe) -> MappedRwLockWriteGuard<T>
	where
		T: 'static + Send + Sync,
	{
		universe.comp_mut(self.as_entity())
	}

	pub fn bypass_get_in_universe<'r>(
		self,
		universe: &ExclusiveUniverse<'r>,
	) -> MappedRwLockReadGuard<'r, T>
	where
		T: 'static + Send + Sync + BypassExclusivity,
	{
		universe.bypass_comp(self.as_entity())
	}

	pub fn bypass_get_mut_in_universe<'r>(
		self,
		universe: &ExclusiveUniverse<'r>,
	) -> MappedRwLockWriteGuard<'r, T>
	where
		T: 'static + Send + Sync + BypassExclusivity,
	{
		universe.bypass_comp_mut(self.as_entity())
	}
}

impl<T> DebugLifetimeWrapper for SingleEntity<T> {
	fn as_debug_lifetime(me: Self) -> DebugLifetime {
		me.entity.lifetime
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
			archetype: self.id(),
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

	pub fn spawn_with_universe<L: DebugLabel>(
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
		if cfg!(debug_assertions) && entity.archetype.id != self.id {
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

	pub fn despawn_and_extract_with_universe(
		&mut self,
		cx: &mut ExclusiveUniverse,
		entity: Entity,
	) -> M
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

// === Bundle === //

pub trait Bundle: Sized {
	type Context<'a>;

	fn attach(self, cx: Self::Context<'_>, target: Entity);

	fn detach(cx: Self::Context<'_>, target: Entity) -> Self;

	fn attach_auto_cx(self, cx: &mut ExclusiveUniverse, target: Entity);

	fn detach_auto_cx(cx: &mut ExclusiveUniverse, target: Entity) -> Self;
}

#[derive(Debug, Copy, Clone, Default)]
pub struct SingleBundle<T>(pub T);

impl<T: 'static + Send + Sync> Bundle for SingleBundle<T> {
	type Context<'a> = &'a mut Storage<T>;

	fn attach(self, storage: Self::Context<'_>, target: Entity) {
		storage.add(target, self.0);
	}

	fn detach(storage: Self::Context<'_>, target: Entity) -> Self {
		Self(storage.try_remove(target).unwrap())
	}

	fn attach_auto_cx(self, cx: &mut ExclusiveUniverse, target: Entity) {
		cx.storage_mut::<T>().add(target, self.0);
	}

	fn detach_auto_cx(cx: &mut ExclusiveUniverse, target: Entity) -> Self {
		Self(cx.storage_mut::<T>().try_remove(target).unwrap())
	}
}

impl<T: 'static + Send + Sync> BuildableArchetype for SingleBundle<T> {}

#[macro_export]
macro_rules! bundle {
	($(
		$(#[$attr_meta:meta])*
		$vis:vis struct $name:ident {
			$(
				$(#[$field_meta:meta])*
				$field_vis:vis $field:ident: $ty:ty
			),*
			$(,)?
		}
	)*) => {$(
		$(#[$attr_meta])*
		$vis struct $name {
			$(
				$(#[$field_meta])*
				$field_vis $field: $ty
			),*
		}

		impl $crate::Bundle for $name {
			type Context<'a> = ($(&'a mut $crate::Storage<$ty>,)*);

			#[allow(unused)]
			fn attach(self, ($($field,)*): Self::Context<'_>, target: $crate::Entity) {
				$( $field.add(target, self.$field); )*
			}

			#[allow(unused)]
			fn detach(($($field,)*): Self::Context<'_>, target: $crate::Entity) -> Self {
				$( let $field = $field.try_remove(target).unwrap(); )*

				Self { $($field),* }
			}

			#[allow(unused)]
			fn attach_auto_cx(self, cx: &mut $crate::ExclusiveUniverse, target: $crate::Entity) {
				$( cx.storage_mut::<$ty>().add(target, self.$field); )*
			}

			#[allow(unused)]
			fn detach_auto_cx(cx: &mut $crate::ExclusiveUniverse, target: $crate::Entity) -> Self {
				$( let $field = cx.storage_mut::<$ty>().try_remove(target).unwrap(); )*

				Self { $($field),* }
			}
		}
	)*};
}

pub use bundle;
