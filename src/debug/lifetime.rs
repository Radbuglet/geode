use std::{borrow::Borrow, cmp::Ordering, fmt, hash, num::NonZeroU64};

use super::label::{DebugLabel, ReifiedDebugLabel};

// === Global === //

type LifetimeSlot = &'static SlotData;

#[derive(Debug)]
struct SlotData(parking_lot::Mutex<SlotDataInner>);

#[derive(Debug)]
struct SlotDataInner {
	gen: NonZeroU64,
	deps: usize,
	curr_name: ReifiedDebugLabel,
	dead_name: ReifiedDebugLabel,
}

mod db {
	use std::{cell::RefCell, num::NonZeroU64};

	use parking_lot::Mutex;

	use super::{LifetimeSlot, SlotData, SlotDataInner};

	use crate::util::pool::{GlobalPool, LocalPool};

	const POOL_BLOCK_SIZE: usize = 1024;

	static GLOBAL_POOL: GlobalPool<LifetimeSlot> = GlobalPool::new();

	thread_local! {
		static LOCAL_POOL: RefCell<LocalPool<LifetimeSlot>> = const { RefCell::new(LocalPool::new()) };
	}

	pub(super) fn alloc_slot() -> LifetimeSlot {
		LOCAL_POOL.with(|local_pool| {
			let mut local_pool = local_pool.borrow_mut();

			local_pool.acquire(&GLOBAL_POOL, || {
				let values = (0..POOL_BLOCK_SIZE)
					.map(|_| {
						SlotData(Mutex::new(SlotDataInner {
							gen: NonZeroU64::new(1).unwrap(),
							deps: 0,
							curr_name: None,
							dead_name: None,
						}))
					})
					.collect::<Vec<_>>()
					.leak();

				values.iter().collect()
			})
		})
	}

	pub(super) fn free_slot(slot: LifetimeSlot) {
		LOCAL_POOL.with(|local_pool| {
			local_pool
				.borrow_mut()
				.release(&GLOBAL_POOL, POOL_BLOCK_SIZE, slot);
		});
	}
}

// === Lifetime === //

#[derive(Copy, Clone)]
pub struct Lifetime {
	slot: LifetimeSlot,
	gen: NonZeroU64,
}

impl hash::Hash for Lifetime {
	fn hash<H: hash::Hasher>(&self, state: &mut H) {
		(self.slot as *const SlotData).hash(state);
		self.gen.hash(state);
	}
}

impl Eq for Lifetime {}

impl PartialEq for Lifetime {
	fn eq(&self, other: &Self) -> bool {
		(self.slot as *const SlotData) == other.slot && self.gen == other.gen
	}
}

impl Lifetime {
	pub fn new<L: DebugLabel>(name: L) -> Self {
		let curr_name = name.reify();

		let slot = db::alloc_slot();
		let mut slot_guard = slot.0.lock();
		slot_guard.curr_name = curr_name;

		Self {
			slot,
			gen: slot_guard.gen,
		}
	}

	pub fn is_alive(self) -> bool {
		self.gen == self.slot.0.lock().gen
	}

	pub fn is_condemned(self) -> bool {
		!self.is_alive()
	}

	pub fn try_inc_dep(self) -> bool {
		let mut slot_guard = self.slot.0.lock();

		if slot_guard.gen != self.gen {
			return false;
		}

		slot_guard.deps = slot_guard.deps.checked_add(1).unwrap_or_else(|| {
			panic!(
				"Marked too many dependencies on `Lifetime` with name {:?}.",
				slot_guard.curr_name,
			)
		});

		true
	}

	pub fn inc_dep(self) {
		if !self.try_inc_dep() {
			log::error!(
				"Attempted to increment the dependency counter on the dead lifetime {:?}.",
				self
			);
		}
	}

	pub fn dec_dep(self) {
		let mut slot_guard = self.slot.0.lock();

		if slot_guard.gen != self.gen {
			// (ignored to reduce spam a bit)
			return;
		}

		slot_guard.deps = slot_guard.deps.checked_sub(1).unwrap_or_else(|| {
			panic!(
				"Decremented dependency counter of `Lifetime` with name {:?} more times than it was incremented.",
				slot_guard.curr_name,
			)
		});
	}

	pub fn try_destroy(self) -> bool {
		let mut slot_guard = self.slot.0.lock();

		// Ensure that the lifetime is still alive
		if slot_guard.gen != self.gen {
			return false;
		}

		// See if we're disconnecting the lifetime from any of its dependencies.
		if slot_guard.deps > 0 {
			log::error!(
				"Disconnected lifetime with name {:?} from {} dependenc{}.",
				slot_guard.curr_name,
				slot_guard.deps,
				if slot_guard.deps > 0 { "ies" } else { "y" }
			);
		}

		// Reset its state
		slot_guard.gen = slot_guard.gen.saturating_add(1);
		slot_guard.deps = 0;
		slot_guard.dead_name = slot_guard.curr_name.take();

		// Release the slot
		if slot_guard.gen.get() != u64::MAX {
			drop(slot_guard);
			db::free_slot(self.slot);
		} else {
			log::error!(
				"A given `Lifetime` was somehow used more than `u64::MAX` times and is being leaked. \
				 How long-running is this application?"
			);
			// (leak the slot)
		}

		true
	}

	pub fn destroy(self) {
		if !self.try_destroy() {
			log::error!(
				"Attempted to destroy lifetime {:?}, which was already destroyed.",
				self
			);
		}
	}

	pub fn debug_name(self) -> LifetimeName {
		LifetimeName(self)
	}

	fn fmt_lifetime_name(self, slot_guard: &SlotDataInner) -> &str {
		let local_gen = self.gen.get();
		let curr_gen = slot_guard.gen.get();

		let name = if local_gen == curr_gen {
			Some(&slot_guard.curr_name)
		} else if local_gen == curr_gen - 1 {
			Some(&slot_guard.dead_name)
		} else {
			None
		};

		match name {
			Some(Some(name)) => name,
			Some(None) => "<name unspecified>",
			None => "<name unavailable>",
		}
	}
}

impl fmt::Debug for Lifetime {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let slot_guard = self.slot.0.lock();

		f.debug_struct("Lifetime")
			.field("name", &self.fmt_lifetime_name(&slot_guard))
			.field("is_alive", &(slot_guard.gen == self.gen))
			.finish_non_exhaustive()
	}
}

#[derive(Copy, Clone)]
pub struct LifetimeName(pub Lifetime);

impl fmt::Debug for LifetimeName {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let slot_guard = self.0.slot.0.lock();
		fmt::Debug::fmt(self.0.fmt_lifetime_name(&slot_guard), f)
	}
}

impl fmt::Display for LifetimeName {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let slot_guard = self.0.slot.0.lock();
		f.write_str(self.0.fmt_lifetime_name(&slot_guard))
	}
}

// === DebugLifetime === //

#[allow(dead_code)]
mod debug_impl {
	use super::*;

	#[derive(Debug, Copy, Clone)]
	pub struct DebugLifetime(Lifetime);

	impl DebugLifetime {
		pub const IS_ENABLED: bool = true;

		pub fn new<L: DebugLabel>(name: L) -> Self {
			Self(Lifetime::new(name))
		}

		pub fn from_lifetime(lifetime: Lifetime) -> Self {
			Self(lifetime)
		}

		pub fn is_possibly_alive(self) -> bool {
			self.0.is_alive()
		}

		pub fn is_condemned(self) -> bool {
			!self.is_possibly_alive()
		}

		pub fn inc_dep(self) {
			self.0.inc_dep();
		}

		pub fn dec_dep(self) {
			self.0.dec_dep();
		}

		pub fn destroy(self) {
			self.0.destroy();
		}

		pub fn raw(self) -> Option<Lifetime> {
			Some(self.0)
		}
	}

	impl From<Lifetime> for DebugLifetime {
		fn from(lifetime: Lifetime) -> Self {
			Self::from_lifetime(lifetime)
		}
	}
}

#[allow(dead_code)]
mod release_impl {
	use super::*;

	#[derive(Debug, Copy, Clone)]
	pub struct DebugLifetime {
		_private: (),
	}

	impl DebugLifetime {
		pub const IS_ENABLED: bool = false;

		pub fn new<L: DebugLabel>(name: L) -> Self {
			let _ = name;

			Self { _private: () }
		}

		pub fn from_lifetime(lifetime: Lifetime) -> Self {
			let _ = lifetime;

			Self { _private: () }
		}

		pub fn is_possibly_alive(self) -> bool {
			true
		}

		pub fn is_condemned(self) -> bool {
			false
		}

		pub fn inc_dep(self) {}

		pub fn dec_dep(self) {}

		pub fn destroy(self) {}

		pub fn raw(self) -> Option<Lifetime> {
			None
		}
	}

	impl From<Lifetime> for DebugLifetime {
		fn from(lifetime: Lifetime) -> Self {
			Self::from_lifetime(lifetime)
		}
	}
}

#[cfg(debug_assertions)]
pub use debug_impl::*;

#[cfg(not(debug_assertions))]
pub use release_impl::*;

impl Eq for DebugLifetime {}

impl PartialEq for DebugLifetime {
	fn eq(&self, _other: &Self) -> bool {
		true
	}
}

impl hash::Hash for DebugLifetime {
	fn hash<H: hash::Hasher>(&self, _state: &mut H) {}
}

impl Ord for DebugLifetime {
	fn cmp(&self, _other: &Self) -> Ordering {
		Ordering::Equal
	}
}

impl PartialOrd for DebugLifetime {
	fn partial_cmp(&self, _other: &Self) -> Option<Ordering> {
		Some(Ordering::Equal)
	}
}

// === Wrapper traits === //

pub trait DebugLifetimeWrapper: Copy {
	fn as_debug_lifetime(me: Self) -> DebugLifetime;

	fn is_possibly_alive(self) -> bool {
		Self::as_debug_lifetime(self).is_possibly_alive()
	}

	fn is_condemned(self) -> bool {
		Self::as_debug_lifetime(self).is_condemned()
	}

	fn as_dependent(self) -> Dependent<Self> {
		self.into()
	}
}

pub trait LifetimeWrapper: DebugLifetimeWrapper {
	fn as_lifetime(me: Self) -> Lifetime;

	fn is_alive(self) -> bool {
		Self::as_lifetime(self).is_alive()
	}

	fn filter_alive(self) -> Option<Self> {
		if self.is_alive() {
			Some(self)
		} else {
			None
		}
	}
}

pub trait DestructibleLifetime: DebugLifetimeWrapper {
	fn destroy(self);
}

impl DebugLifetimeWrapper for Lifetime {
	fn as_debug_lifetime(me: Self) -> DebugLifetime {
		me.into()
	}
}

impl LifetimeWrapper for Lifetime {
	fn as_lifetime(me: Self) -> Lifetime {
		me
	}
}

impl DestructibleLifetime for Lifetime {
	fn destroy(self) {
		self.destroy();
	}
}

impl DebugLifetimeWrapper for DebugLifetime {
	fn as_debug_lifetime(me: Self) -> DebugLifetime {
		me
	}
}

impl DestructibleLifetime for DebugLifetime {
	fn destroy(self) {
		self.destroy();
	}
}

// === Wrapper Objects === //

#[derive(Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct OwnedLifetime<L: DestructibleLifetime>(L);

impl<L: DestructibleLifetime> OwnedLifetime<L> {
	pub fn new(lifetime: L) -> Self {
		Self(lifetime)
	}

	pub fn get(&self) -> L {
		self.0
	}

	pub fn defuse(self) -> L {
		let lt = self.get();
		std::mem::forget(self);
		lt
	}
}

impl<L: DestructibleLifetime> Borrow<L> for OwnedLifetime<L> {
	fn borrow(&self) -> &L {
		&self.0
	}
}

impl<L: DestructibleLifetime> From<L> for OwnedLifetime<L> {
	fn from(value: L) -> Self {
		Self::new(value)
	}
}

impl<L: DestructibleLifetime> Drop for OwnedLifetime<L> {
	fn drop(&mut self) {
		self.get().destroy();
	}
}

#[derive(Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct Dependent<L: DebugLifetimeWrapper>(L);

impl<L: DebugLifetimeWrapper> Dependent<L> {
	pub fn new(lifetime: L) -> Self {
		L::as_debug_lifetime(lifetime).inc_dep();
		Self(lifetime)
	}

	pub fn get(&self) -> L {
		self.0
	}

	pub fn into_inner(self) -> L {
		let lifetime = self.0;
		drop(self);
		lifetime
	}
}

impl<L: DebugLifetimeWrapper> Borrow<L> for Dependent<L> {
	fn borrow(&self) -> &L {
		&self.0
	}
}

impl<L: DebugLifetimeWrapper> Clone for Dependent<L> {
	fn clone(&self) -> Self {
		Self::new(self.get())
	}
}

impl<L: DebugLifetimeWrapper> From<L> for Dependent<L> {
	fn from(value: L) -> Self {
		Self::new(value)
	}
}

impl<L: DebugLifetimeWrapper> Drop for Dependent<L> {
	fn drop(&mut self) {
		L::as_debug_lifetime(self.0).dec_dep();
	}
}
