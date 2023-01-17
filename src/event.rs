use std::{
	any::type_name,
	borrow::{Borrow, BorrowMut},
	collections::HashMap,
	fmt, mem,
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

// === OpaqueBox === //

#[derive(Clone, Hash, Eq, PartialEq, Ord, PartialOrd, Default)]
pub struct OpaqueBox<T: ?Sized>(pub Box<T>);

impl<T: ?Sized> OpaqueBox<T> {
	pub fn new(v: T) -> Self
	where
		T: Sized,
	{
		Box::new(v).into()
	}

	pub fn from_box(b: Box<T>) -> Self {
		b.into()
	}
}

impl<T: ?Sized> fmt::Debug for OpaqueBox<T> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct(format!("OpaqueBox<{}>", type_name::<T>()).as_str())
			.finish_non_exhaustive()
	}
}

impl<T: ?Sized> From<Box<T>> for OpaqueBox<T> {
	fn from(value: Box<T>) -> Self {
		Self(value)
	}
}

impl<T: ?Sized> Borrow<Box<T>> for OpaqueBox<T> {
	fn borrow(&self) -> &Box<T> {
		&self.0
	}
}

impl<T: ?Sized> BorrowMut<Box<T>> for OpaqueBox<T> {
	fn borrow_mut(&mut self) -> &mut Box<T> {
		&mut self.0
	}
}

impl<T: ?Sized> Borrow<T> for OpaqueBox<T> {
	fn borrow(&self) -> &T {
		&self.0
	}
}

impl<T: ?Sized> BorrowMut<T> for OpaqueBox<T> {
	fn borrow_mut(&mut self) -> &mut T {
		&mut self.0
	}
}

impl<T: ?Sized> Deref for OpaqueBox<T> {
	type Target = T;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

impl<T: ?Sized> DerefMut for OpaqueBox<T> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.0
	}
}
