# Geode To-Do

##### Entity Model

- [x] Singleton bundles.
- [ ] Add support for late-initialized `bundle!` components.
- [ ] Expose `Archetype::spawn_push`, `Archetype::spawn_in_slot`, and `Archetype::iter`.
- [ ] Allow `EventQueueIter` to be reiterated and polled on individual archetypes.
- [ ] Implement `MappedStorage` and the `StorageView` trait.

##### Multi-Threading

- [ ] Allow more direct manipulation of `Storage` (specifically, expose runs and allow users to get an `UnsafeCell<T>` version of the storage given a mutable reference to it).
- [ ] Implement more storage types:
  - [ ] Single-threaded ref-celling for multi-borrow
  - [ ] Sharding at the archetype level
  - [ ] Rayon integration
- [ ] Implement regular scheduling:
  - [ ] Implement `Scheduler`
  - [ ] Implement `AsyncProvider`
  - [ ] Implement an `async` version of `Universe`
  - [ ] Implement pool-based future executor

##### Debug

- [ ] Implement lifetime stats.
- [ ] Improve debug messages:
  - [ ] Add names to `Dependent` objects and log them out on disconnection
  - [ ] Add custom error hooks for the debugger
  - [ ] Log backtraces on error
  - [ ] Warnings for other forms of misuse (e.g. not flushing the universe)
  - [ ] Better/more consistent messages for everything else

##### Extensions

- [x] Improve `compost`:
  - [ ] Allow unlimited `Deref` chains.
  - [ ] Allow for opt-in increases to max arity.
- [ ] Implement `Signal`.
- [ ] Expose `WeakEntity` and `WeakArchetype`.
- [ ] Implement `ArchetypeAnnotator`.

##### Publishing

- [ ] Publish a stable interface for `compost`.
- [ ] Perform code review and write unit tests.
- [ ] Document library and publish.
