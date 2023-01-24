# Geode

A Scheduling-Flexible, Powerful ECS

## Overview

There are three major components to Geode:

- **Component Storages**, which provide mechanisms for creating entities, attaching components to them, and accessing those components through either random access or querying.
- **Universes**, which provide mechanisms for automatically creating and accessing global state (e.g. storages and some archetypes) and passing that state around.
- **Events**, which provide mechanisms for queueing up events on entities, connecting signals, defining debug-printable closures, and scheduling function graphs with state dependencies for concurrent execution.

### Component Storages

**Archetypes** and **entities** are the building blocks of Geode applications.

An `Entity` is a 64 bit `Copy`'able identifier for some logical object. This could be a game object like a player or a zombie, or it could be something more abstract like a signal.

Every `Entity` is associated with *exactly one* `Archetype` and they will never change archetype throughout their lifetimes. Archetypes are collections of entities with the same set of components and entities within a given archetype are extremely efficient to iterate over.

There is no need for a `Universe` to begin using these objects. You can create the archetype like any other Rust container and spawn entities from it directly.

```rust
use geode::Archetype;

// Create an archetype with a debug label.
//
// The generic parameter is a marker type used to provide
// some level of type-safety in function signatures requesting
// an archetype of a certain kind. This type can be casted at will
// using the `cast_marker_xxx` methods.
let mut players = Archetype::<()>::new("players");

// Spawn an entity within that archetype.
//
// Once again, this method takes a debug label for the entity.
let my_player = players.spawn("my player");

// We'll also create an archetype for our zombies.
let mut zombies = Archetype::<()>::new("zombies");
let my_zombie = zombies.spawn("my zombie");
```

We can attach state to a given `Entity` using a `Storage`. Once again, these act like regular Rust containers:

```rust
use geode::Storage;

let mut positions = Storage::<[f32; 3]>::new();
let mut names = Storage::<String>::new();
let mut ai_goals = Storage::<Option<Entity>>::new();

// Different archetypes can have different sets of components.
positions.add(my_player, [0., 0., 0.]);
names.add(my_player, "The Player".to_string());

positions.add(my_zombie, [1., 2., 4.]);
ai_goals.add(my_zombie, Some(my_player));  // `Entity` is `Copy`
```

Storages expose a `HashMap`-like interface and allow for efficient random-access:

```rust
println!("Player {my_player:?} is at {:?}", positions[my_player]);

positions[my_player][1] = 3.;

if let Some(position) = positions.get(my_zombie) {
    println!("Zombie {my_zombie:?} has a position component.");
}
```

You can also iterate over the components of entities within a single archetype:

```rust
use geode::Query;

// You can query over storages one archetype at a time.
for (player, pos, name) in (&mut positions, &names).query_in(players.id()) {
    pos[1] -= 9.8;
    println!("{player:?} with name {name:?} has been updated.");
}

for (zombie, &ai_goal) in (&ai_goals,).query_in(zombies.id()) {
    if let Some(goal) = ai_goals {
        println!("{zombie:?} is chasing {goal:?} at {:?}", positions[goal]);
    }
}

let archetypes_needing_updating = vec![
    players.id(),  // `ArchetypeId` is also `Copy`.
    zombies.id(),
];

for &arch_id in &archetypes_needing_updating {
    for (target, pos) in (&mut positions,).query_in(arch_id) {
        // ...
    }
}
```

==TODO: Explain why we don't have a `query_all` method/why using it would be an anti-pattern.==

Deletions are handled manually. You must remove all uses of an entity before removing it from its parent archetype. Additionally, there is no way to check whether an `Entity` is still alive at runtime. These limitations exist to avoid creating a global store of entity generations, which would involve locking or flushing of global state. However, you can always write automated deletion mechanisms in userland.

```rust
// Components must be removed from an entity before it can be
// despawned from the archetype lest a UAF warning be raised.
positions.remove(my_zombie);
ai_goals.remove(my_zombie);
zombies.despawn(my_zombie);

// There is no flushing required for spawning or despawning entities.
for (zombie, &pos) in (&positions,).query_in(zombies.id()) {
    unreachable!();  // all our zombies are gone.
}
```

Although there is no way to check whether an `Entity` is still alive at runtime, debug builds will still produce use-after-free (UAF) warnings. This is done using an additional 4-`usize`s worth of metadata in every `Entity` to efficiently check whether the entity is still alive. This adds little runtime overhead but is nonetheless debug-only because of the space overhead.

You can interact with these lifetimes using the `debug::lifetime` module. The most frequently used objects are `Dependent` and the `is_possibly_alive` and `is_condemned` methods from the `LifetimeLike` trait.

```rust
use geode::{Dependent, lifetime::LifetimeLike};

let player_ref = Dependent::new(my_player);

// Uncommenting this section would produce a UAF on player despawn
// because `player_ref` still depends on the `Entity`.
// positions.remove(my_player);
// names.remove(my_player);
// players.despawn(my_player);

drop(player_ref);

// Now that the `player_ref` has been dropped, these run just fine.
positions.remove(my_player);
names.remove(my_player);
players.despawn(my_player);

// You can also check whether the `Entity` has been condemned
// i.e. is guaranteed to be dead. This method will always return
// `false` for debug builds.
let is_condemned = my_player.is_condemned();

// This value is `true` when debug assertions are turned on and
// false if this is a release build.
assert!(is_condemned == cfg!(debug_assertions));
```

#### Archetype Maps

==TODO: Document archetype maps.==

#### Owned Entity

==TODO: Document owned entities.==

#### Bundles

==TODO: Document bundles.==

#### Archetype Groups

==TODO: Document archetype groups.==

### Universes

**Universes**, like *worlds* in traditional entity-component-systems, are a store of global state. However, unlike ECS worlds, universes typically only store `Storages`—that's it! It is very rare, although it is allowed, to store global state in the same way that you would store singletons as resources in an ECS world.

We can get away with this in Geode because, unlike a traditional ECS, Geode encourages you to define systems as regular functions receiving their context through their arguments. For example, instead of storing a timer as a resource so that game systems have access to it, the object in charge of handling that scene could pass the scene entity to its list of dependencies and let them acquire the timer from there. This ensures that, for example, we can create multiple scenes with multiple different timers in a given world without having to replace every instance of a timer resource with a timer component on a scene entity.

Although the previous paragraph would suggest that all forms of global state are anti-patterns—encouraging a design where the `Universe` is omitted entirely—we still have good reason to make most `Storages` into singletons. In putting all components of a given type into a single storage, it becomes trivial to access the component: just acquire the corresponding storage from the universe and index into it. Compare that to a multi-storage approach where there is no differentiation between a component being in a different storage or just not being there at all!

You get these benefits without losing opportunities for multi-threading thanks to the `ShardedStorage` wrapper, which allows you to access components from different archetypes concurrently on different threads so long as you can prove exclusive access to that archetype through a mutable reference to the `Archetype`.

There is also the occasional argument for placing `Archetypes` into the `Universe`—usually convenience since there really isn't a good architectural argument for why putting them elsewhere can be harmful. Doing this, however, is quite rare and support is somewhat limited out of the box (e.g. you can't easily add metadata to the archetype in its constructor without going through a locked universe resource). It is much more common to see `Archetypes` stored directly in other components, whether that be directly or through an `ArchetypeGroup`.

But that's enough rambling! Here's how to use a `Universe` to access resources:

```rust
use geode::{
    Universe,
    universe::{
        BuildableArchetype,
        BuildableResource,
        BuildableResourceRw,
    },
};
use std::time::Instant;

let mut universe = Universe::new();

// Accessing storages is easy!
let mut positions = universe.storage_mut::<[f32; 3]>();
let mut names = universe.storage_mut::<String>();
let mut ai_goals = universe.storage_mut::<Option<Entity>>();

// Accessing global archetypes is also quite easy. All you need
// is a marker type.
struct PlayerArchMarker;

impl BuildableArchetype for PlayerArchMarker {
    fn create(_universe: &Universe) -> Archetype<Self> {
        Archetype::new("player archetype")
    }
}

let mut players = universe.archetype_mut::<PlayerArchMarker>();
let my_player = players.spawn("my player");

// It is typical to use a bundle as the marker type.
use geode::bundle;

bundle! {
    pub struct ZombieBundle {
        pub position: [f32; 3],
        ai_goal: Option<Entity>,
    }
}

// Omitting the `create` method gives you a default `create` method that
// produces a plain `Archetype` instance with the debug name set to the
// type name of the bundle.
impl BuildableArchetype for ZombieBundle {}

let mut zombies = universe.archetype_mut::<ZombieBundle>();
let my_zombie = players.spawn_with(
    "my zombie",
    (&mut positions, &mut ai_goals),
    ZombieBundle {
        position: [0.0, 1.0, 0.0],
        ai_goal: Some(my_player),
    },
);

// We can also access global resources, although this use-case is
// far less frequent.
struct AppStart(pub Instant);

impl BuildableResource for AppStart {
    fn create(_universe: &Universe) -> Self {
        Self(Instant::now())
    }
}

let app_start = universe.resource::<AppStart>();

// Read-write components are also supported. These are equivalent to
// calling `universe.resource::<RwLock<T>>().try_lock_xxx().unwrap()`.
struct Counter(pub u32);

impl BuildableResourceRw for Counter {
    fn create(_universe: &Universe) -> Self {
        Self(0)
    }
}

let mut counter = universe.resource_mut::<Counter>();
*counter += 1;

drop(counter);

println!("The counter is now {}.", *universe.resource_ref::<Counter>)();
```

#### Exclusive Universes

When passing the `Universe` around—especially when passing it to dynamically dispatched function handlers—it can be pretty tricky to keep track of which resources have already been borrowed. Luckily, Geode provides a useful wrapper around `Universes` to make them much safer without compromising too much of their flexibility.

`ExclusiveUniverse<'r>` is a wrapper around a `&'r Universe` that asserts that it is the only such reference to that `Universe`. Methods expecting exclusive access over the `Universe` and all of its components can request a `&mut ExclusiveUniverse`. These behave almost exactly like references to a `Universe` and, indeed, `ExclusiveUniverse` immutably dereferences to a `Universe`. `ExclusiveUniverse` differentiates itself from a `&mut Universe`, however, by the way in which they allow carefully-selected carveouts for bypassing this exclusivity.

For example, say you want to have a game scene call into a bunch of entity processing systems. Ideally, you'd want to give these systems an `&mut ExclusiveUniverse`. However, to access these closures, you need access to the scene's corresponding `GameSceneState`. It's pretty obvious that those systems won't try to access that component so, ideally, this should be made safe to borrow.

And, indeed, it can be. Any resource, storage component, or archetype bundle for which it should be safe to bypass this exclusivity restriction can implement the `BypassExclusivity` trait. This allows you to call the special `bypass_xxx` methods, which return references in such a way that you can still pass a `&mut ExclusiveUniverse<'r>` along while still holding on to them.

```rust
use geode::{ExclusiveUniverse, BypassExclusivity, OpaqueBox};

struct SomeEngineService {
    // ...
}

struct PlaySceneState {
    systems: Vec<OpaqueBox<dyn FnMut(&mut BypassExclusivity)>>,
}

impl BypassExclusivity for PlaySceneState {}

fn process_scene(universe: &mut ExclusiveUniverse, engine: Entity, scene: Entity) {
    // You can use the `Universe`'s regular borrowing methods to borrow
    // non-`BypassExclusivity` components...
    {
        let mut my_service = &mut universe.storage_mut::<SomeEngineService>()[engine];
        my_service.do_something();
        // Note, however, that this borrow must be dropped before reborrowing
        // `ExclusiveUniverse` mutably again.
    }

    // You can use the `Universe`'s `bypass_xxx` methods to bypass the exclusivity
    // constraint.
    let mut scene_state = &mut universe.storage_mut::<PlaySceneState>()[scene];

    for system in &mut scene_state.systems {
        // Notice how we can keep on borrowing `scene_state` while passing
        // a `&mut ExclusiveUniverse` reference?
        system(&mut *universe);
    }
}
```

#### Context Tuples

==TODO==

## Events

==TODO==

#### Multi-Threading

==TODO==

## Examples

==TODO==

