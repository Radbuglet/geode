use std::{any::type_name, collections::HashMap, mem, num::NonZeroU32, vec};

use derive_where::derive_where;

use crate::{
	debug::lifetime::{DebugLifetime, Dependent},
	entity::hashers::ArchetypeBuildHasher,
	ArchetypeId, BypassExclusivity, Entity, ExclusiveUniverse,
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

pub trait FuncMethodInject<I> {
	const INJECTOR: I;
}

// TODO: Replace this `impl` with a more flexible one allowing custom resolution.
impl<T: 'static + Send + Sync + BypassExclusivity>
	FuncMethodInject<
		for<'injected> fn(
			&mut &mut ExclusiveUniverse<'injected>,
			&mut Entity,
		) -> parking_lot::MappedRwLockWriteGuard<'injected, T>,
	> for T
{
	const INJECTOR: for<'injected> fn(
		&mut &mut ExclusiveUniverse<'injected>,
		&mut Entity,
	) -> parking_lot::MappedRwLockWriteGuard<'injected, T> = todo!();
}

// === `func!` macro === //

#[doc(hidden)]
pub mod macro_internal {
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
				self [$($inj_name:ident: $inj:ty),* $(,)?]
				$(, $para_name:ident: $para:ty)* $(,)?
			)
		$(where $($where_token:tt)*)?
	) => {
		$crate::func! {
			$(#[$attr_meta])*
			$vis fn $name
				< $($($generic),*)? >
				< 'injected, $($($($fn_lt),*)?)? >
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
			pub fn new_method_mut<__Receiver, __Func>(handler: __Func) -> Self
			where
				__Receiver: ?Sized + 'static + $crate::event::FuncMethodInject<
					for<
						'injected
						$($(
							$(,$fn_lt)*
						)?)?
					> fn(
						$(&mut $inj),*
					) -> $crate::parking_lot::MappedRwLockWriteGuard<'injected, __Receiver>,
					// TODO: Open up to more types of guards.
				>,
				__Func: 'static + $crate::event::macro_internal::Send + $crate::event::macro_internal::Sync +
					for<
						'injected
						$($(
							$(,$fn_lt)*
						)?)?
					> Fn(
						&mut __Receiver,
						$($inj,)*
						$($para,)*
					),
			{
				Self::new(move |$(mut $inj_name,)* $($para_name,)*| {
					let mut __guard = __Receiver::INJECTOR($(&mut $inj_name,)*);

					handler(&mut *__guard, $($inj_name,)* $($para_name,)*);
				})
			}

			#[allow(unused)]
			pub fn new_method_ref<__Receiver, __Func>(handler: __Func) -> Self
			where
				__Receiver: ?Sized + 'static + $crate::event::FuncMethodInject<
					for<
						'injected
						$($(
							$(,$fn_lt)*
						)?)?
					> fn(
						$(&mut $inj),*
					) -> $crate::parking_lot::MappedRwLockReadGuard<'injected, __Receiver>,
					// TODO: Open up to more types of guards.
				>,
				__Func: 'static + $crate::event::macro_internal::Send + $crate::event::macro_internal::Sync +
					for<
						'injected
						$($(
							$(,$fn_lt)*
						)?)?
					> Fn(
						&__Receiver,
						$($inj,)*
						$($para,)*
					),
			{
				Self::new(move |$(mut $inj_name,)* $($para_name,)*| {
					let __guard = __Receiver::INJECTOR($(&mut $inj_name,)*);

					handler(&*__guard, $($inj_name,)* $($para_name,)*);
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
			pub fn new<__Func>(handler: __Func) -> Self
			where
				__Func: 'static + $($(for<$($fn_lt),*>)?)? Fn($($para),*) + $crate::event::macro_internal::Send + $crate::event::macro_internal::Sync,
			{
				Self {
					_ty: ($($($crate::event::macro_internal::PhantomData::<$generic>,)*)?),
					handler: $crate::event::macro_internal::Arc::new(handler),
				}
			}
		}

		impl<
			__Func:
				'static +
					$($(for<$($fn_lt),*>)?)? Fn($($para),*) +
					$crate::event::macro_internal::Send +
					$crate::event::macro_internal::Sync
			$(, $($generic),*)?
		> $crate::event::macro_internal::From<__Func> for $name $(<$($generic),*>)?
		$(where
			$($where_token)*
		)? {
			fn from(handler: __Func) -> Self {
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
