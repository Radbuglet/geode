# Geode To-Do

##### Universe

- [x] Improve dependency injection:
  - [x] Improve combined `unpack!` syntax.
  - [x] Implement `compost`-level tuple combination, especially for `rest`.
  - [x] Update `unpack!` to use new `decompose!` macro.
  - [x] Clean up `unpack!` macro forms.
  - [x] Remove component limit on `unpack!` now that cons-lists have proper support?
  - [ ] Implement `provider_from_tuple` again.
- [x] Additional `decompose!` features:
  - [x] Allow users to decompose temporaries
  - [x] Expose the functionality to make something a cons-list without the need for `decompose!(...x => ()).1` jank.
  - [ ] Allow unlimited `Deref` chains.
  - [ ] Allow for opt-in increases to max arity.
  - [ ] Publish these features.
- [x] Implement more alias methods in `Universe`.
- [x] Improve task executors:
  - [x] Flush universe between tasks.
  - [x] Remove special case for `Universe` in `Provider` by adding a `get_frozen` method.
  - [x] Allow users to provide an input context to the task execution pass.
- [x] Improve task executors part 2:
  - [x] Implement `CleanProvider`.
  - [x] Add support for nesting `CleanProvider` instances.
  - [x] Implement `TaskHandler`.
  - [x] Implement `TaskQueue` such that it a) has all the proper convenience methods and b) allows users to add tasks to it within their own executors.
  - [ ] Remove universe's queue.
- [ ] Remove special cases for universe systems:
  - [ ] Extract `ArchetypeAnnotator`.
  - [ ] Allow users to register archetype deletion hooks as custom metadata keys. This can be done safely because deletions are only processed on `flush`.
  - [ ] Optimize tag querying, add `TagId`-namespaced archetype metadata.
- [ ] Improve the universe resource system:
  - [ ] Implement `Universe`-global `EventQueues`.
  - [ ] Add support for non-auto-initializable `Universe` resources.
  - [ ] Extract as well?

##### Entity Model

- [ ] Expose `WeakEntity` and `WeakArchetype`.
- [ ] Add support for late-initialized `bundle!` components.
- [ ] Expose `Archetype::spawn_push`, `Archetype::spawn_in_slot`, and `Archetype::iter`.
- [ ] Singleton bundles.
- [ ] Allow `EventQueueIter` to be reiterated and polled on individual archetypes.
- [ ] Implement `MappedStorage` and the `StorageView` trait.
- [ ] Implement `Signal`.

##### Multi-Threading

- [ ] Allow more direct manipulation of `Storage` (specifically, expose runs and allow users to get an `UnsafeCell<T>` version of the storage given a mutable reference to it).
- [ ] Implement more storage types:
  - [ ] Single-threaded ref-celling for multi-borrow
  - [ ] Sharding at the archetype level
  - [ ] Rayon integration
- [ ] Implement regular scheduling:
  - [ ] Implement `Scheduler`
  - [ ] Implement `AsyncProvider`
  - [ ] Implement an `async` version of `unpack!`
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

- [ ] Perform code review and write unit tests.
- [ ] Document library and publish.

## Design Concerns

This design is similar enough to more traditional object-oriented `GameObject` implementations that most design concerns end up being resolved by just copying those solutions. Thus, to begin thinking about design concerns, it is most useful to begin by listing where our design deviates from the object-oriented `GameObject` design:

1. Users have to manually manage data access (archetypes and storages).
   1. They have to decide how to pass this data to the appropriate system at the appropriate time.
   2. They have to decide how to structure their data.
2. Users have to deal with more complex dispatch mechanisms such as `EventQueues` and archetype querying.

##### Context Passing

We'll begin by considering the problem of state passing. This problem is precisely the problem a more-restrictive traditional ECS tries to solve with the notion of a "system." While restricting users to a strict ECS-style scheduler feels like an attractive solution to ensure that our library is well designed (we would immediately benefit from the corpus of knowledge on traditional ECS'), this decision would likely reduce the overall utility of this library. Indeed, the *raison d'être* of Geode is to provide flexibility in the way that they execute systems to projects like Crucible—which expose their own form of plugin dispatch which cannot be easily modeled by a flat executor.

Essentially, we need a pattern where *nested dispatch* is encouraged. In other words, we need an ECS-style model where systems are encouraged rather than discouraged from calling into other nested subsystems. At the same time, we want to bring over the following useful properties provided by an ECS' scheduler:

1. It should be easy to change the origin of a given piece of state. For example, it should be possible to combine or split the same of several components and update the systems depending on those components without getting stopped by borrow violations.
2. If the user wants to forfeit the above property for the sake of performance/predictability, they should be notified, at compile time, whether a borrow violation exists.

There are four types of dispatches supported by the current design:

1. **Universe Tasks:** These allow a handler to knowingly acquire the *entire* application context and receive the very strong guarantee that it is the only handler doing so at a given time.
2. **Context Tuples:** These allow users to immediately call a function with a compile-time-known set of components without any queueing. Context passing in these dispatches is resolved statically.
3. **Providers:** These allow users to immediately call a function with a type-erased set of components without any queueing.
4. **Async Providers and Thread Pools:** These implement the ECS-style parallel executor we're all used to.

Universe tasks perfectly satisfy the first property but are quite heavyweight. Context tuples perfectly satisfy the second property and are extremely cheap.

Providers (both synchronous and asynchronous) are a bit more complicated. On their own, they satisfy neither property and are extremely dangerous because of it. However, you can emulate the guarantees provided by universe tasks through informal contracts. Because most `Providers` and `AsyncProviders` are used to dispatch child task sets (e.g. a game engine dispatching a game scene handler dispatching a plugin handler dispatching an entity handler, etc...), by making clear which components are internal state that shouldn't be back-referenced, child handlers can more-or-less behave like universe tasks.

We may try to enforce this type of contract statically in the future.

##### State Structuring

State structuring involves a number of additional responsibilities with varying degrees of foreignness to the regular object-oriented pattern.

Most familiar will be the creation of archetypes and tags. Archetypes map fairly neatly onto the object-oriented idea of homogenous sets of identically typed entities. This is also the place where Geode improves the most on the flexibility afforded by an traditional ECS. Therefore, I am not too worried about the utility of these.

What concerns me more, however, is storage structuring. Having multiple storage instances dedicated to a single type—especially if those storages exist in the `Universe` as well—could easily cause confusion as to which instance stores which entity.

Luckily, creating multiple `Storages` is not the only way to achieve sharding. Indeed, given that entities are already organized by their archetype within a storage, it is not too difficult to implement archetype-level sharding. The benefits of this are immense:

1. All storages are managed by the `Universe`, allowing sharding-unaware routines to access the state directly.
2. Users get very explicit errors when they attempt to access an entity outside of the current shard.
3. Archetypes are already quite easy to reason about for the reasons stated above.
4. Archetypes are first-class citizens in Geode so the flexibility they afford will likely translate over to sharded environments as well.

We therefore consider unmanaged `Storages` to be an anti-pattern; there almost never is a good reason to use them.

##### Dispatch Mechanisms

There are a few questions that need to be answered in order to use `EventQueues` effectively:

1. How do we get the `EventQueue` to the handler?
2. How do we decide when an `EventQueue` should be dispatched?

The first question could get a bit tricky if multiple layers of execution decide to provide their own version of the same `EventQueue`. Therefore, I find it quite beneficial to store every `EventQueue` in the `Universe` and provide `Universe::push_to_queue<T>(EventQueue<T>)` and `Universe::take_queue<T>() -> EventQueueIter<T>` methods to access it.

==TODO: Think about these problems a bit more.==

##### Summarizing Foot-Guns

Geode has a few foot-guns:

- The overuse of `EventHandler`.
- Using multiple `Storages` for a given component instead of using proper sharding.
- Using a raw `Archetype<T>` instead of an `ArchetypeHandle<T>`.
- Assuming that callers will not call `get_frozen` on a given component.
