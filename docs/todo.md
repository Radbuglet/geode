# Geode To-Do

##### Universe

- [x] Improve dependency injection:
  - [x] Improve combined `unpack!` syntax.
  - [x] Implement `compost`-level tuple combination, especially for `rest`.
  - [x] Update `unpack!` to use new `decompose!` macro.
  - [x] Clean up `unpack!` macro forms.
  - [x] Remove component limit on `unpack!` now that cons-lists have proper support?
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
  - [ ] Implement a scratch space and give tasks access to it.
  - [ ] Make sure that `Providers` "thaw" the components of their ancestors that they froze to avoid unexpected freezing.
- [ ] Allow users to register archetype deletion hooks as custom metadata keys. This can be done safely because deletions are only processed on `flush`.
- [ ] Optimize tag querying, add `TagId`-namespaced archetype metadata.

##### Entity Model

- [ ] Expose `WeakEntity` and `WeakArchetype`.
- [ ] Add support for late-initialized `bundle!` components.
- [ ] Expose `Archetype::spawn_push`, `Archetype::spawn_in_slot`, and `Archetype::iter`.
- [ ] Singleton bundles.
- [ ] Allow `EventQueueIter` to be reiterated and polled on individual archetypes.

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

##### Obviated

- There were concerns about **single component objects** and a desire to make them their own entity type. While we might see some value in defining `SingletonBundle`, the machinery required to optimize for this special case is too much, especially considering that this choice would limit your ability to do other things later. Honestly, this is more a context verbosity problem than an architecture problem.
- There were some concerns about the implementation of signals. Specifically, **where do we attach the handle state**? To the target handler? In its own entity? This is actually the same exact issue we saw with `kotae-core` and, like in that issue, we decided that it is best to handle these at a case-by-case basis while building abstractions that could support both.
- There was an argument to **remove archetype metadata and tags**. However,
  - These things serve important purposes that need to be as easy as possible:
    - Tags are exceedingly useful for making this design traditional-ECS-compliant. We can create root-level systems that iterate over groups of archetypes incredibly easily. Just think of all the things we'd have to manually remove otherwise.
    - Metadata can be used to acquire additional behavior descriptors for entity type without faffing around with `ArchetypeMaps`, which would pose the exact same verbosity problem as the removed-tags proposal.

  - This type of special casing may feel a bit weird—we force `Entities` to handle deletion manually—but we need this special casing to allow for lazy `BuildableResource` creation, which requires tightly-knit `Universe::flush()` integration. All the extra convenience is just a way to keep the interface consistent.
  - Additionally, once the archetypal metadata destructor task proposal goes through, tags could be implemented entirely in userland.
-  There was an argument to replace the entire context passing mechanism with **a regular ECS scheduler, maybe with nesting**. The arguments for this were: we can reduce the size of context tuples and we could implement sharding more easily. This isn't so much an argument for reduction, however, as it is an argument more for bite-sized systems and inline `Storages `than against any existing design features. We can easily support this extension using our proposed multithreading model—there's no need to tear everything down.
  - It's also not a good counterargument to imply that direct execution is an anti-pattern. Direct execution is a relatively-heavyweight mechanism for shaping macro-level execution as one desires, which is necessary for a lot of game features which cannot be directly bootstrapped by a traditional ECS (e.g. user plugin loading and unloading, which requires its own scheduling mechanism). Indeed, enabling this type of execution for the sake of Crucible is the *raison-d'être* of Geode.

- There was a concern that **task queues could obfuscate side-effects**. However, tasks are already expected to be written in such a way that they are global-order independent (i.e. they should expect other tasks to be queued up before them and should therefore not make state assumptions about the objects they're manipulating), so this really is only a concern for deletion ordering. We can ensure that these cases don't happen by neatly dividing the engine's tasks into phases, ensuring that `dispatch_tasks` happens before a deletion dispatch and `flush` happens after.

- One concern is the **ease of use of the library**. Is it really just as easy to write it in `geode` as it would be to write it in `kotae-core`? Specifically, how many steps does it take to add something?

  `geode`'s and `kotae-core`'s designs are effectively isomorphic. The power of `goede` comes from the way in which we can further optimize this base OOP design and turn it into a veritable data-oriented implementation. This is done with a few features:

  1. Explicit event queues (users decide where these come from and when they get executed)
  2. Explicit archetype creation (users decide the logical owner of an archetype; users have to create their own component bundles manually)
  3. Explicit system configuration (users decide when queries are launched and the tags needed to do so)

  These are the main sources of any additional complexity a `geode` user may face. Thus, the question becomes: How easy is it to define a new event queue? How easy is it to define a new archetype? How easy is it to link all of these things together?

  In my opinion, this is not a concern of `geode`. Archetypes and tags are certainly not new to OOP designs (they are often used in about the same place that an entity list might be used) and, in OOP designs, it is the responsibility of the engine designer—not the object-model designer—to implement contextually-appropriate mechanisms for configuring all of this. Indeed, there is no global solution to these mechanisms (although traditional ECS' have proposed unified task schedulers—an abstraction worth potentially adding to `geode`) so being too overbearing about the precise implementation of this mechanism would be detrimental to our library.

  The main discomfort with this rebuttal stems from the fear that definition work will need to be duplicated in multiple places, making system isolation difficult and potentially dangerous (e.g. an entity archetype is created and used but is never registered in the appropriate registry). This scenario can be prevented by ensuring that archetypes are created alongside their entry in the registry (e.g. the archetype constructor takes a reference to the registry) which, again, is not an uncommon pattern for descriptor objects in OOP.


##### Active

- There is still a *lot* of context passing. This causes problems where:
  - **We have super-high-arity tuples containing context.** This is problematic because we often reuse the same context tuple among many services. Thus, while we can easily move one tuple to another, we still have to repeat signatures. Additionally, we currently have a limit of 12 elements in these tuples and this limit is not expandable. Finally, it can be hard to think of exactly which components we'll need for a given system, making the process of writing these tuples a bit tedious.
  - There is concern that these global context passes (see: storages) could enforce **unnecessary data dependencies** where (automatic?) sharding could make things more efficient.
- There are quite a few instances of unenforced rules:
  - Late initialization of bundle components being skipped.
  - `Provider` component lists not be appropriate.
  - Entity and archetype types. It isn't always clear which dispatch/deletion mechanism should be used and migrating between mechanisms can be quite difficult.

