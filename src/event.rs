use std::{any::type_name, collections::HashMap, fmt, mem, num::NonZeroU32, vec};

use derive_where::derive_where;

use crate::{
	context::CleanProvider,
	debug::lifetime::{DebugLifetime, Dependent},
	entity::hashers::ArchetypeBuildHasher,
	util::type_id::NamedTypeId,
	ArchetypeId, Entity, Provider,
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

// === EventHandler === //

pub struct EventHandler<E>(Box<dyn EventHandlerTrait<E>>);

impl<E: 'static> fmt::Debug for EventHandler<E> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct(format!("EventHandler<{}>", type_name::<E>()).as_str())
			.field("handler_ty", &self.0.type_id())
			.finish_non_exhaustive()
	}
}

impl<E: 'static> Clone for EventHandler<E> {
	fn clone(&self) -> Self {
		Self(self.0.cloned())
	}
}

impl<E: 'static> EventHandler<E> {
	pub fn new<F>(f: F) -> Self
	where
		F: 'static + Fn(&Provider, E) + Send + Sync + Clone,
	{
		Self(Box::new(f))
	}

	pub fn process(&self, cx: &Provider, event: E) {
		self.0.process(cx, event);
	}
}

trait EventHandlerTrait<E>: 'static + Send + Sync {
	fn cloned(&self) -> Box<dyn EventHandlerTrait<E>>;
	fn process(&self, cx: &Provider, event: E);
	fn type_id(&self) -> NamedTypeId;
}

impl<E, F> EventHandlerTrait<E> for F
where
	E: 'static,
	F: 'static + Fn(&Provider, E) + Send + Sync + Clone,
{
	fn cloned(&self) -> Box<dyn EventHandlerTrait<E>> {
		Box::new(self.clone())
	}

	fn process(&self, cx: &Provider, event: E) {
		self(cx, event)
	}

	fn type_id(&self) -> NamedTypeId {
		NamedTypeId::of::<Self>()
	}
}

// === GenericTaskQueue === //

#[derive(Debug)]
#[derive_where(Default)]
pub struct GenericTaskQueue<T> {
	task_stack: Vec<T>,
	tasks_to_add: Vec<T>,
}

impl<T> GenericTaskQueue<T> {
	pub fn new() -> Self {
		Self::default()
	}

	pub fn push(&mut self, task: T) {
		// These are queued in a separate buffer and moved into the main buffer during `next_task`
		// to ensure that tasks are pushed in an intuitive order.
		self.tasks_to_add.push(task);
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

impl<T> Drop for GenericTaskQueue<T> {
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

// === TaskQueue === //

pub type TaskQueue = GenericTaskQueue<TaskHandler<()>>;

impl TaskQueue {
	pub fn run_tasks(&mut self, cx: &mut CleanProvider) {
		while let Some(mut task) = self.next_task() {
			task.process(cx, ());
		}
	}
}

#[derive_where(Debug, Clone; E: 'static)]
pub struct TaskHandler<E> {
	pub raw: EventHandler<E>,
}

impl<E: 'static> TaskHandler<E> {
	pub fn new<F>(f: F) -> Self
	where
		F: 'static + Fn(&Provider, E) + Send + Sync + Clone,
	{
		Self {
			raw: EventHandler::new(f),
		}
	}

	pub fn new_clean<F>(f: F) -> Self
	where
		F: 'static + Fn(&mut CleanProvider) + Send + Sync + Clone,
	{
		Self::new(move |cx, _| f(&mut CleanProvider::unchecked_new(cx)))
	}

	pub fn process(&mut self, cx: &mut CleanProvider, event: E) {
		self.raw.process(&cx, event);
	}
}
