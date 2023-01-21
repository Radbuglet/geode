# Geode To-Do

##### Entity Model

- [x] Singleton bundles.
- [ ] Add support for late-initialized and nested `bundle!` components.
- [ ] Expose `Archetype::spawn_push`, `Archetype::spawn_in_slot`, `Archetype::len`, and `Archetype::iter`.
- [ ] Allow `EventQueueIter` to be reiterated and polled on individual archetypes.
- [ ] Implement a universe-friendly `SyncArchetypeMap`.

##### Convenience Extensions

- [x] Allow `bundles` to spawn from an `ExclusiveUniverse`.
  - [x] Replace this feature with a method in the `Universe` to make it slightly safer.
- [x] Implement dispatch utils:
  - [x] Mechanisms to make writing delegates easier (e.g. a `func!` macro)
  - [x] Method conversions in `func`
  - [x] Add return types to `func`
  - [ ] Ability to take `func` objects statically
- [x] Implement `MappedStorage` and `StorageView` traits.
- [ ] Improve `compost`:
  - [ ] Allow unlimited `Deref` chains.
  - [ ] Allow users to define custom primary and backup data sources
  - [ ] Allow users to `decompose!` a `Universe` as a fallback
  - [ ] Allow for opt-in increases to max arity.
- [ ] Implement `EntityMap` and `ArchetypeMap`.

##### Multi-Threading

- [ ] Allow more direct manipulation of `Storage` (specifically, expose runs and allow users to get an `UnsafeCell<T>` version of the storage given a mutable reference to it).
- [ ] Clean up queries to better accomodate the change above.
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

##### Publishing

- [ ] Publish a stable interface for `compost`.
- [ ] Perform code review and write unit tests.
- [ ] Document library and publish.

## Unresolved Questions

1. Sharded storages have to be special-cased in the `Universe`. How could we generalize these mechanisms to other forms of temporaries?
2. `BypassExclusivity` is a pretty broad trait. Could we make it more fine to prevent even more forms of misuse? Should we force regular and `BypassExclusivity` resources to use different methods to aid in refactoring?
3. The `ExclusiveUniverse` model can force quite a bit of reborrowing. Is there a way around this?
4. What about `Storage` reborrows? If we're just giving a borrow-only `Storage` view to another user, we should be able to reuse the reference we already resolved.
