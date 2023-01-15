# Geode Scheduling

##### The Problem

We would like to implement the following interface:

```rust
pub struct Scheduler { ... }

impl Scheduler {
  pub async fn acquire(&self, deps: &[(NamedTypeId, Mutability)]);
  pub fn unacquire(&self, deps: &[(NamedTypeId, Mutability)]);
}
```

A trivial solution to this problem could be to enforce a locking order and represent every single component as an `RwLock`. However, this implementation could lead to a scenario where:

- Thread `1` is trying to acquire components `A` and `B`.
- Thread `2` is trying to acquire components `A` and `C`.
- Thread `3` is actively holding `A`.
- Thread `4` is actively holding `B`.
- Thread `5` is actively holding `C`.
- Thread `3` releases `A` and thread `2` holds onto it.
- Thread `4` releases `B`. While thread `1` could theoretically run right now, it is blocked on thread `2`'s acquisition.
- Thread `5` holds onto `C` for a long time. Thread `1` is artificially starved.

There is no clever (non PGO'd) heuristic to avoid this scenario—we need the `Scheduler` to take an active role in, well, scheduling.

A simple scheduler could involve storing "blocked dependency" counter for every request in the system. Every time a set of components are acquired, the counter for every dependent request would be increased. Every time a component is released, every request blocked on that component would have its counter decremented. A random thread with a zero counter would gain access to the components it was requesting and all other dependency counters would be incremented.

The running time of this scheme to completion, unfortunately, is $O(n^2)$ w.r.t the number of tasks depending on a given component. This could be fine for small $n$ but the size of this variable can be hard to predict given the variable time taken by every task. Can we do better?

##### Proof of Concept

We'll start by thinking of solutions to scenarios where every component is locked exclusively and extend it to XOR mutability later.

The first data-structure that popped to my mind was the [hierarchical bit-set](https://docs.rs/hibitset/latest/hibitset/index.html). Hibitsets allow you to:

- Store a sparse array of bits where...
- You can find the first set bit in $O(1)$ and can...
- Take binary `and`s and `or`s of multiple sets while keeping the aforementioned operation $O(1)$.

If we assign an index to every task (which is easily done using a free list), we can record whether a given component is *not* depended upon by a given task. To determine which tasks could possibly start at a given time, take the bit-set of all the tasks managed by the scheduler and take its bitwise-`and` w.r.t all the `SchedulerComponent.no_dependency` bit-sets whose `is_available` flag is enabled.

Here's a code sample:

```rust
use std::{any::TypeId, collections::HashMap, task::Waker};

use hibitset::{BitSet, BitSetLike};

pub struct BitSetAndMany<'a>(Vec<&'a BitSet>);

impl BitSetLike for BitSetAndMany<'_> {
    // ...

	fn layerN(&self, i: usize) -> usize {
		self.0
			.iter()
			.fold(usize::MAX, |accum, set| accum & set.layerN(i))
	}
}

pub struct Scheduler {
	tasks: Vec<Waker>,
	task_set: BitSet,
	comps: HashMap<TypeId, Component>,
}

#[derive(Default)]
struct Component {
	is_available: bool,
	without_dep_set: BitSet,
}

impl Scheduler {
	pub fn alloc_task(&mut self, waker: Waker) -> u32 {
		todo!("omitted")
	}

	pub fn wake_task_and_dealloc(&mut self, id: u32) {
		todo!("omitted")
	}

	pub fn try_acquire_now(&mut self, comps: &[TypeId]) -> bool {
		todo!("omitted")
	}

	pub fn acquire_later(&mut self, comps: &[TypeId], waker: Waker) {
		// Allocate an ID for the task and record its waker
		let task_id = self.alloc_task(waker);

		// Update the appropriate `without_dep_set` bit-sets.
		for &comp in comps {
			let comp = self.comps.entry(comp).or_insert(Default::default());

			// When a task goes unused, all its `without_dep_set` bits are set.
			comp.without_dep_set.remove(task_id);
		}
	}

	pub fn unlock(&mut self, comps: &[TypeId]) {
		// Mark the components as available.
		for comp in comps {
			let Some(comp) = self.comps.get_mut(comp) else { continue; };

			comp.is_available = true;
		}

		// Poll for new availabilities
		while self.poll_one() {}
	}

	pub fn poll_one(&mut self) -> bool {
		// Determine the sets needed to filter down the task-set to just the
        // tasks that can run.
		let sets_to_and = [&self.task_set]
			.into_iter()
			.chain(self.comps.values().filter_map(|comp| {
				if !comp.is_available {
					Some(&comp.without_dep_set)
				} else {
					None
				}
			}))
			.collect();

		// Find the first task that can run
		let can_run_set = BitSetAndMany(sets_to_and);
		let Some(task) = can_run_set.iter().next() else {
			return false
		};

		// Add back all the `without_dep_set` bits and mark those comps as
        // unavailable.
		for comp in self.comps.values_mut() {
			// When a task goes unused, all its `without_dep_set` bits are set.
			if comp.without_dep_set.add(task) {
				comp.is_available = false;
			}
		}

		// Wake up the task and remove it from the scheduler
		self.wake_task_and_dealloc(task);
		true
	}
}
```

As you can see, the running time of each of these operations is indeed linear w.r.t the number of component types.

Huzzah!
