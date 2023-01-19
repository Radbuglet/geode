use std::{
	any::type_name,
	collections::HashMap,
	mem,
	num::NonZeroU32,
	ops::{Deref, DerefMut},
	vec,
};

use derive_where::derive_where;

use crate::{
	debug::lifetime::{DebugLifetime, Dependent},
	entity::hashers::ArchetypeBuildHasher,
	ArchetypeId, Entity,
};

// === Aliases === //

#[derive(Debug, Clone, Default)]
pub struct EntityDestroyEvent;

pub type DestroyQueue = EventQueue<EntityDestroyEvent>;

// === EventQueue === //

#[derive(Debug, Clone)]
#[derive_where(Default)]
pub struct EventQueue<E> {
	runs: HashMap<NonZeroU32, (Dependent<DebugLifetime>, Vec<Event<E>>), ArchetypeBuildHasher>,
	maybe_recursively_dispatched: bool,
}

impl<E> EventQueue<E> {
	pub fn new() -> Self {
		Self::default()
	}

	pub fn push(&mut self, target: Entity, event: E) {
		let run = self.runs.entry(target.arch.id).or_insert_with(|| {
			self.maybe_recursively_dispatched = true;
			(Dependent::new(target.arch.lifetime), Vec::new())
		});

		run.1.push(Event {
			slot: target.slot,
			lifetime: Dependent::new(target.lifetime),
			event,
		});
	}

	pub fn flush_all(&mut self) -> impl Iterator<Item = EventQueueIter<E>> {
		mem::take(&mut self.runs)
			.into_iter()
			.map(|(arch_id, (arch_lt, events_list))| {
				EventQueueIter(
					ArchetypeId {
						id: arch_id,
						lifetime: arch_lt.into_inner(),
					},
					events_list.into_iter(),
				)
			})
	}

	pub fn flush_in(&mut self, archetype: ArchetypeId) -> EventQueueIter<E> {
		EventQueueIter(
			archetype,
			self.runs
				.remove(&archetype.id)
				.map_or(Vec::new(), |(_, events)| events)
				.into_iter(),
		)
	}

	pub fn maybe_recursively_dispatched(&mut self) -> bool {
		mem::replace(&mut self.maybe_recursively_dispatched, false)
	}

	pub fn is_empty(&self) -> bool {
		self.runs.is_empty()
	}

	pub fn has_remaining(&self) -> bool {
		!self.is_empty()
	}
}

impl<E> Drop for EventQueue<E> {
	fn drop(&mut self) {
		if !self.runs.is_empty() {
			let leaked_count = self.runs.values().map(|(_, run)| run.len()).sum::<usize>();

			log::error!(
				"Leaked {leaked_count} event{} from {}",
				if leaked_count == 1 { "" } else { "s" },
				type_name::<Self>()
			);
		}
	}
}

#[derive(Debug, Clone)]
struct Event<E> {
	slot: u32,
	lifetime: Dependent<DebugLifetime>,
	event: E,
}

impl<E> Event<E> {
	fn into_tuple(self, arch: ArchetypeId) -> (Entity, E) {
		(
			Entity {
				slot: self.slot,
				lifetime: self.lifetime.get(),
				arch,
			},
			self.event,
		)
	}
}

#[derive(Debug, Clone)]
pub struct EventQueueIter<E>(ArchetypeId, vec::IntoIter<Event<E>>);

impl<E> EventQueueIter<E> {
	pub fn arch(&self) -> ArchetypeId {
		self.0
	}
}

impl<E> Iterator for EventQueueIter<E> {
	type Item = (Entity, E);

	fn next(&mut self) -> Option<Self::Item> {
		self.1.next().map(|e| e.into_tuple(self.0))
	}

	fn size_hint(&self) -> (usize, Option<usize>) {
		self.1.size_hint()
	}

	fn count(self) -> usize {
		self.1.count()
	}
}

impl<E> ExactSizeIterator for EventQueueIter<E> {}

impl<E> DoubleEndedIterator for EventQueueIter<E> {
	fn next_back(&mut self) -> Option<Self::Item> {
		self.1.next_back().map(|e| e.into_tuple(self.0))
	}
}

// === TaskQueue === //

#[derive(Debug)]
#[derive_where(Default)]
pub struct TaskQueue<T> {
	task_stack: Vec<T>,
	tasks_to_add: Vec<T>,
}

impl<T> TaskQueue<T> {
	pub fn new() -> Self {
		Self::default()
	}

	pub fn push(&mut self, task: impl Into<T>) {
		// These are queued in a separate buffer and moved into the main buffer during `next_task`
		// to ensure that tasks are pushed in an intuitive order.
		self.tasks_to_add.push(task.into());
	}

	pub fn next_task(&mut self) -> Option<T> {
		// Move all tasks from `tasks_to_add` to `task_stack`. This flips their order, which is
		// desireable.
		self.task_stack.reserve(self.tasks_to_add.len());
		while let Some(to_add) = self.tasks_to_add.pop() {
			self.task_stack.push(to_add);
		}

		// Now, pop off the next task to be ran.
		self.task_stack.pop()
	}

	pub fn clear_capacities(&mut self) {
		self.task_stack = Vec::new();
		self.tasks_to_add = Vec::new();
	}
}

impl<T> Drop for TaskQueue<T> {
	fn drop(&mut self) {
		let remaining = self.task_stack.len() + self.tasks_to_add.len();

		if remaining > 0 {
			log::warn!(
				"Leaked {} task{} on the `TaskQueue`.",
				remaining,
				if remaining == 1 { "" } else { "s" },
			);
		}
	}
}

// === `func!` traits === //

pub trait FuncMethodInjectorRef<T: ?Sized> {
	type Guard<'a>: Deref<Target = T>;
	type Injector;

	const INJECTOR: Self::Injector;
}

pub trait FuncMethodInjectorMut<T: ?Sized> {
	type Guard<'a>: DerefMut<Target = T>;
	type Injector;

	const INJECTOR: Self::Injector;
}

pub mod injectors {
	use super::*;
	use crate::{BypassExclusivity, ExclusiveUniverse};

	// === ByExclusiveStorage === //

	#[derive(Debug, Copy, Clone, Default)]
	pub struct ByExclusiveStorage;

	impl<T: 'static + Send + Sync + BypassExclusivity> FuncMethodInjectorRef<T> for ByExclusiveStorage {
		type Guard<'a> = parking_lot::MappedRwLockReadGuard<'a, T>;
		type Injector = for<'i> fn(&mut &mut ExclusiveUniverse<'i>, &mut Entity) -> Self::Guard<'i>;

		const INJECTOR: Self::Injector = |cx, me| cx.bypass_comp(*me);
	}

	impl<T: 'static + Send + Sync + BypassExclusivity> FuncMethodInjectorMut<T> for ByExclusiveStorage {
		type Guard<'a> = parking_lot::MappedRwLockWriteGuard<'a, T>;
		type Injector = for<'i> fn(&mut &mut ExclusiveUniverse<'i>, &mut Entity) -> Self::Guard<'i>;

		const INJECTOR: Self::Injector = |cx, me| cx.bypass_comp_mut(*me);
	}
}

// === `func!` macro === //

#[doc(hidden)]
pub mod macro_internal {
	use super::{FuncMethodInjectorMut, FuncMethodInjectorRef};
	use std::ops::DerefMut;

	pub trait FuncMethodInjectorRefGetGuard<T: ?Sized> {
		type GuardHelper<'a>: Deref<Target = T>;
	}

	impl<G, T> FuncMethodInjectorRefGetGuard<T> for G
	where
		T: ?Sized,
		G: FuncMethodInjectorRef<T>,
	{
		type GuardHelper<'a> = G::Guard<'a>;
	}

	pub trait FuncMethodInjectorMutGetGuard<T: ?Sized> {
		type GuardHelper<'a>: DerefMut<Target = T>;
	}

	impl<G, T> FuncMethodInjectorMutGetGuard<T> for G
	where
		T: ?Sized,
		G: FuncMethodInjectorMut<T>,
	{
		type GuardHelper<'a> = G::Guard<'a>;
	}

	pub use std::{
		clone::Clone,
		convert::From,
		fmt,
		marker::{PhantomData, Send, Sync},
		ops::Deref,
		stringify,
		sync::Arc,
	};
}

#[macro_export]
macro_rules! func {
	(
		$(#[$attr_meta:meta])*
		$vis:vis fn $name:ident
			$(
				<$($generic:ident),* $(,)?>
				$(<$($fn_lt:lifetime),* $(,)?>)?
			)?
			(
				&$inj_lt:lifetime self [$($inj_name:ident: $inj:ty),* $(,)?]
				$(, $para_name:ident: $para:ty)* $(,)?
			)
		$(where $($where_token:tt)*)?
	) => {
		$crate::func! {
			$(#[$attr_meta])*
			$vis fn $name
				< $($($generic),*)? >
				< $inj_lt, $($($($fn_lt),*)?)? >
				(
					$($inj_name: $inj,)*
					$($para_name: $para,)*
				)
			$(where $($where_token)*)?
		}

		impl$(<$($generic),*>)? $name $(<$($generic),*>)?
		$(where
			$($where_token)*
		)? {
			#[allow(unused)]
			pub fn new_method_ref<Injector, Receiver, Func>(_injector: Injector, handler: Func) -> Self
			where
				Injector: 'static + $crate::event::macro_internal::FuncMethodInjectorRefGetGuard<Receiver>,
				Injector: $crate::event::FuncMethodInjectorRef<
					Receiver,
					Injector = for<
						$inj_lt
						$($(
							$(,$fn_lt)*
						)?)?
					> fn(
						$(&mut $inj),*
					) -> Injector::GuardHelper<$inj_lt>>,
				Receiver: ?Sized + 'static,
				Func: 'static + $crate::event::macro_internal::Send + $crate::event::macro_internal::Sync +
				for<
					$inj_lt
					$($(
						$(,$fn_lt)*
					)?)?
				> Fn(
					&Receiver,
					$($inj,)*
					$($para,)*
				),
			{
				Self::new(move |$(mut $inj_name,)* $($para_name,)*| {
					let guard = Injector::INJECTOR($(&mut $inj_name,)*);

					handler(&*guard, $($inj_name,)* $($para_name,)*);
				})
			}

			#[allow(unused)]
			pub fn new_method_mut<Injector, Receiver, Func>(_injector: Injector, handler: Func) -> Self
			where
				Injector: 'static + $crate::event::macro_internal::FuncMethodInjectorMutGetGuard<Receiver>,
				Injector: $crate::event::FuncMethodInjectorMut<
					Receiver,
					Injector = for<
						$inj_lt
						$($(
							$(,$fn_lt)*
						)?)?
					> fn(
						$(&mut $inj),*
					) -> Injector::GuardHelper<$inj_lt>>,
				Receiver: ?Sized + 'static,
				Func: 'static + $crate::event::macro_internal::Send + $crate::event::macro_internal::Sync +
				for<
					$inj_lt
					$($(
						$(,$fn_lt)*
					)?)?
				> Fn(
					&mut Receiver,
					$($inj,)*
					$($para,)*
				),
			{
				Self::new(move |$(mut $inj_name,)* $($para_name,)*| {
					let mut guard = Injector::INJECTOR($(&mut $inj_name,)*);

					handler(&mut *guard, $($inj_name,)* $($para_name,)*);
				})
			}
		}
	};
	(
		$(#[$attr_meta:meta])*
		$vis:vis fn $name:ident
			$(
				<$($generic:ident),* $(,)?>
				$(<$($fn_lt:lifetime),* $(,)?>)?
			)?
			($($para_name:ident: $para:ty),* $(,)?)
		$(where $($where_token:tt)*)?
	) => {
		$(#[$attr_meta])*
		$vis struct $name $(<$($generic),*>)?
		$(where
			$($where_token)*
		)? {
			_ty: ($($($crate::event::macro_internal::PhantomData<$generic>,)*)?),
			// TODO: Optimize the internal representation to avoid allocations for context-less handlers.
			handler: $crate::event::macro_internal::Arc<
				dyn
					$($(for<$($fn_lt),*>)?)?
					Fn($($para),*) +
						$crate::event::macro_internal::Send +
						$crate::event::macro_internal::Sync
			>,
		}

		impl$(<$($generic),*>)? $name $(<$($generic),*>)?
		$(where
			$($where_token)*
		)? {
			#[allow(unused)]
			pub fn new<Func>(handler: Func) -> Self
			where
				Func: 'static + $($(for<$($fn_lt),*>)?)? Fn($($para),*) + $crate::event::macro_internal::Send + $crate::event::macro_internal::Sync,
			{
				Self {
					_ty: ($($($crate::event::macro_internal::PhantomData::<$generic>,)*)?),
					handler: $crate::event::macro_internal::Arc::new(handler),
				}
			}
		}

		impl<
			Func:
				'static +
					$($(for<$($fn_lt),*>)?)? Fn($($para),*) +
					$crate::event::macro_internal::Send +
					$crate::event::macro_internal::Sync
			$(, $($generic),*)?
		> $crate::event::macro_internal::From<Func> for $name $(<$($generic),*>)?
		$(where
			$($where_token)*
		)? {
			fn from(handler: Func) -> Self {
				Self::new(handler)
			}
		}

		impl$(<$($generic),*>)? $crate::event::macro_internal::Deref for $name $(<$($generic),*>)?
		$(where
			$($where_token)*
		)? {
			type Target = dyn
				$($(for<$($fn_lt),*>)?)? Fn($($para),*) +
				$crate::event::macro_internal::Send +
				$crate::event::macro_internal::Sync;

			fn deref(&self) -> &Self::Target {
				&*self.handler
			}
		}

		impl$(<$($generic),*>)? $crate::event::macro_internal::fmt::Debug for $name $(<$($generic),*>)?
		$(where
			$($where_token)*
		)? {
			fn fmt(&self, fmt: &mut $crate::event::macro_internal::fmt::Formatter) -> $crate::event::macro_internal::fmt::Result {
				fmt.write_str("func!::")?;
				fmt.write_str($crate::event::macro_internal::stringify!($name))?;
				fmt.write_str("(")?;
				$(
					fmt.write_str($crate::event::macro_internal::stringify!($para))?;
				)*
				fmt.write_str(")")?;

				Ok(())
			}
		}

		impl$(<$($generic),*>)? $crate::event::macro_internal::Clone for $name $(<$($generic),*>)?
		$(where
			$($where_token)*
		)? {
			fn clone(&self) -> Self {
				Self {
					_ty: ($($($crate::event::macro_internal::PhantomData::<$generic>,)*)?),
					handler: $crate::event::macro_internal::Clone::clone(&self.handler),
				}
			}
		}
	};
}

pub use func;
