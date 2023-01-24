# Geode To-Do

##### Entity Model

- [ ] Add support for late-initialized and nested `bundle!` components.
- [ ] Allow `EventQueueIter` to be reiterated and polled on individual archetypes.
- [ ] Implement some sort of automatic `ArchetypeMap` that is `Universe`-friendly.

##### Convenience Extensions

- [x] Singleton bundles.
- [x] Allow `bundles` to spawn from an `ExclusiveUniverse`.
  - [x] Replace this feature with a method in the `Universe` to make it slightly safer.
- [x] Implement dispatch utils:
  - [x] Mechanisms to make writing delegates easier (e.g. a `func!` macro)
  - [x] Method conversions in `func`
  - [x] Add return types to `func`
  - [ ] Ability to take `func` objects statically
- [x] Implement `MappedStorage` and `StorageView` traits.
- [x] Implement `ArchetypeGroup`.
- [ ] Implement `SingletonMap`.
- [ ] Implement `OwnedEntity`.
- [ ] Improve `compost`:
  - [ ] Allow unlimited `Deref` chains.
  - [ ] Allow users to define custom primary and backup data sources
  - [ ] Allow users to `decompose!` a `Universe` as a fallback
  - [ ] Allow for opt-in increases to max arity.
- [ ] Implement new-types around `EntityMap` and `ArchetypeMap`.

##### Multi-Threading

- [ ] Allow more direct manipulation of `Storage` (specifically, expose runs and allow users to get an `UnsafeCell<T>` version of the storage given a mutable reference to it).
- [ ] Clean up queries to better accommodate the change above.
- [ ] Implement more storage types:
  - [ ] Free reborrowing on pure views
  - [ ] Single-threaded ref-celling for multi-borrow
  - [ ] Sharding at the archetype level
  - [ ] Rayon integration
- [ ] Expose `Archetype::spawn_push`, `Archetype::spawn_in_slot`, `Archetype::len`, and `Archetype::iter`.
- [ ] Implement regular scheduling:
  - [ ] Implement `Scheduler`
  - [ ] Implement `AsyncProvider`
  - [ ] Implement an `async` version of `Universe`
  - [ ] Implement pool-based future executor

##### Debug

- [ ] Optimize `is_alive` checks to be entirely lockless.
- [ ] Implement lifetime stats.
- [ ] Improve debug messages:
  - [ ] Add names to `Dependent` objects and log them out on disconnection
  - [ ] Add custom error hooks for the debugger
  - [ ] Log backtraces on error
  - [ ] Warnings for other forms of misuse (e.g. not flushing the universe)
  - [ ] Better/more consistent messages for everything else

##### Publishing

- [ ] Publish a stable interface for `compost`.
- [ ] Perform code review and write unit tests.
- [ ] Document library and publish.
