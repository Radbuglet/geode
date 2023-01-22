# Geode To-Do

##### Entity Model

- [x] Singleton bundles.
- [ ] Add support for late-initialized and nested `bundle!` components.
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
- [ ] Implement new-types around `EntityMap` and `ArchetypeMap`.

##### Multi-Threading

- [ ] Allow more direct manipulation of `Storage` (specifically, expose runs and allow users to get an `UnsafeCell<T>` version of the storage given a mutable reference to it).
- [ ] Clean up queries to better accomodate the change above.
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

## Unresolved Questions

1. Sharded storages have to be special-cased in the `Universe`. How could we generalize these mechanisms to other forms of temporaries?
2. `BypassExclusivity` is a pretty broad trait. Could we make it more fine to prevent even more forms of misuse? Should we force regular and `BypassExclusivity` resources to use different methods to aid in refactoring?
3. The `ExclusiveUniverse` model can force quite a bit of reborrowing. Is there a way around this?
4. What about `Storage` reborrows? If we're just giving a borrow-only `Storage` view to another user, we should be able to reuse the reference we already resolved.
5. There are a lot of scenarios where a user would like to just have one archetype cover all entities for performance or convenience reasons (e.g. a scene). However, because we tie so much of reference passing and dependency injection to an entity's components, this could very easily back-fire. Is there a way to accomodate the desires for a singleton archetype with the latent need for flexibility?
6. Is there ever an argument for old-Geode-style `Provider` passing or should everything be tied to an `Entity`? Should we support the mechanism? If we don't, can we extend the `Entity` model to provide the same benefits? (mainly the lack of a need to introduce a new archetype and better concurrent access)
7. In general, there is still quite a bit of planning around borrows. We need to think about appropriate `ExclusiveUniverse` schemes, ways of allowing multi-threading, ways to reduce reborrows once all the dependencies are injected, etc.
