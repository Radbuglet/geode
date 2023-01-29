# Geode To-Do

##### Entity Model

- [x] Implement archetype registry in `Universe`
- [x] Implement `WeakArchetype`
- [x] Implement `WeakArchetypeMap`
- [x] Implement universal archetype annotations
- [ ] Implement `WeakEntity` as well
- [ ] Add support for late-initialized and nested `bundle!` components
- [ ] Allow `EventQueueIter` to be reiterated and polled on individual archetypes

##### Convenience Extensions

- [x] Allow `bundles` to spawn from an `ExclusiveUniverse`
  - [x] Replace this feature with a method in the `Universe` to make it slightly safer
- [x] Singleton bundles and entities
  - [x] Move `bundle` to entity
  - [x] Implement `SingleEntity`
  - [x] Implement helper methods to access universe
- [x] Implement dispatch utils:
  - [x] Mechanisms to make writing delegates easier (e.g. a `func!` macro)
  - [x] Method conversions in `func`
  - [x] Add return types to `func`
  - [ ] Ability to take `func` objects statically
- [x] Implement `MappedStorage` and `StorageView` traits
- [ ] Implement new destruction model:
  - [ ] Implement standard destructor traits and delegates
  - [ ] Implement `OwnedEntity`
- [ ] Clean up extension methods
- [ ] Improve `compost`:
  - [ ] Allow unlimited `Deref` chains
  - [ ] Allow users to define custom primary and backup data sources
  - [ ] Allow users to `decompose!` a `Universe` as a fallback
  - [ ] Allow for opt-in increases to max arity

##### Multi-Threading

- [ ] Improve `Storage` flexibility:
  - [x] Implement `&mut Storage<T>` to `&mut Storage<UnsafeCell<T>>` conversion
  - [ ] Implement transparent sharding
  - [ ] Update the query system
  - [ ] Rayon integration
- [ ] Expose `Archetype::spawn_push`, `Archetype::spawn_in_slot`, `Archetype::len`, and `Archetype::iter`
- [ ] Implement regular scheduling:
  - [ ] Implement `Scheduler`
  - [ ] Implement `AsyncProvider`
  - [ ] Implement an `async` version of `Universe`
  - [ ] Implement pool-based future executor

##### Debug

- [ ] Optimize `is_alive` checks to be entirely lockless
- [ ] Implement lifetime stats
- [ ] Improve debug messages:
  - [ ] Add names to `Dependent` objects and log them out on disconnection
  - [ ] Add custom error hooks for the debugger
  - [ ] Log backtraces on error
  - [ ] Warnings for other forms of misuse (e.g. not flushing the universe)
  - [ ] Better/more consistent messages for everything else
  - [ ] Implement new-types around `EntityMap` and `ArchetypeMap`

##### Publishing

- [ ] Publish a stable interface for `compost`
- [ ] Perform code review and write unit tests
- [ ] Document library and publish
