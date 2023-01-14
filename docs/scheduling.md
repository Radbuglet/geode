# Geode Scheduling

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
