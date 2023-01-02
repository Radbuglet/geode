use derive_where::derive_where;
use hibitset::BitSet;
use std::{
	collections::{HashMap, HashSet},
	marker::PhantomData,
	mem::transmute,
	num::NonZeroU32,
};

use parking_lot::Mutex;

use crate::{
	debug::{
		label::{DebugLabel, NO_LABEL},
		lifetime::{DebugLifetime, LifetimeLike, OwnedLifetime},
	},
	util::{no_hash::RandIdGen, ptr::PointeeCastExt},
	Bundle, Dependent,
};

// === Handles === //

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct ArchetypeId {
	pub lifetime: DebugLifetime,
	pub id: NonZeroU32,
}

impl LifetimeLike for ArchetypeId {
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
	pub fn slot_usize(&self) -> usize {
		self.slot as usize
	}
}

impl LifetimeLike for Entity {
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

// === Maps === //

pub mod hashers {
	pub use crate::util::no_hash::NoOpBuildHasher as ArchetypeBuildHasher;
	pub use fnv::FnvBuildHasher as EntityBuildHasher;
}

pub type ArchetypeMap<V> = HashMap<Dependent<ArchetypeId>, V, hashers::ArchetypeBuildHasher>;
pub type ArchetypeSet = HashSet<Dependent<ArchetypeId>, hashers::ArchetypeBuildHasher>;
pub type EntityMap<V> = HashMap<Dependent<ArchetypeId>, V, hashers::EntityBuildHasher>;
pub type EntitySet = HashSet<Dependent<ArchetypeId>, hashers::EntityBuildHasher>;

// === Archetype === //

static ARCH_ID_FREE_LIST: Mutex<Option<RandIdGen>> = Mutex::new(None);

#[derive_where(Debug)]
#[repr(C)]
pub struct Archetype<M: ?Sized = ()> {
	_ty: PhantomData<fn(M) -> M>,
	id: NonZeroU32,
	lifetime: OwnedLifetime<DebugLifetime>,
	slots: Vec<OwnedLifetime<DebugLifetime>>,
	free_slots: BitSet,
}

impl<M: ?Sized> Archetype<M> {
	pub fn new<L: DebugLabel>(name: L) -> Self {
		// Generate archetype ID
		let id = ARCH_ID_FREE_LIST
			.lock()
			.get_or_insert_with(Default::default)
			.alloc();

		// Construct archetype
		Self {
			_ty: PhantomData,
			id,
			lifetime: OwnedLifetime::new(DebugLifetime::new(name)),
			slots: Vec::new(),
			free_slots: BitSet::new(),
		}
	}

	pub fn spawn<L: DebugLabel>(&mut self, name: L) -> Entity {
		// Construct a lifetime
		let lifetime = DebugLifetime::new(name);

		// Allocate a free slot
		let slot = match (&self.free_slots).into_iter().next() {
			Some(slot) => slot,
			None => {
				let slot = self.slots.len() as u32;
				assert_ne!(slot, u32::MAX, "spawned too many entities");

				self.slots.push(OwnedLifetime::new(lifetime));
				slot
			}
		};
		self.free_slots.add(slot);

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

		let _ = self.free_slots.remove(entity.slot);
	}

	pub fn despawn_and_extract(&mut self, cx: M::Context<'_>, entity: Entity) -> M
	where
		M: Bundle,
	{
		let bundle = M::detach(cx, entity);
		self.despawn(entity);
		bundle
	}

	pub fn id(&self) -> ArchetypeId {
		ArchetypeId {
			lifetime: self.lifetime.get(),
			id: self.id,
		}
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
			self.transmute_pointee_ref()
		}
	}

	pub fn cast_marker_mut<N: ?Sized>(&mut self) -> &mut Archetype<N> {
		unsafe {
			// Safety: This struct is `repr(C)` and `N` is only ever used in a `PhantomData`.
			self.transmute_pointee_mut()
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
		ARCH_ID_FREE_LIST
			.lock()
			.get_or_insert_with(Default::default)
			.dealloc(self.id);
	}
}
