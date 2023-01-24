use std::{
	any::{type_name, Any},
	marker::PhantomData,
	mem::{self, transmute},
	ops::Deref,
	sync::{Arc, Weak},
};

use fnv::FnvBuildHasher;
use parking_lot::{
	MappedMutexGuard, MappedRwLockReadGuard, MappedRwLockWriteGuard, Mutex, MutexGuard, RwLock,
	RwLockReadGuard, RwLockWriteGuard,
};

use crate::{
	debug::{label::DebugLabel, lifetime::LifetimeLike},
	entity::hashers,
	func,
	util::{eventual_map::EventualMap, type_id::NamedTypeId},
	Archetype, ArchetypeId, Bundle, Entity, Storage,
};

// === Universe === //

#[derive(Debug, Default)]
pub struct Universe {
	resources: EventualMap<NamedTypeId, dyn Any + Send + Sync, FnvBuildHasher>,
	archetypes: EventualMap<ArchetypeId, ManagedArchetype, hashers::ArchetypeBuildHasher>,
	proxied: Arc<ProxyState>,
}

#[derive(Debug)]
struct ManagedArchetype {
	archetype: Mutex<Archetype>,
	removal_tasks: Mutex<Vec<UniverseArchRemovalTask>>,
}

#[derive(Debug, Default)]
struct ProxyState {
	flush_tasks: Mutex<Vec<UniverseFlushTask>>,
}

impl Universe {
	pub fn new() -> Self {
		Self::default()
	}

	pub fn as_exclusive(&mut self) -> ExclusiveUniverse<'_> {
		ExclusiveUniverse::new(self)
	}

	pub fn as_exclusive_dangerous(&self) -> ExclusiveUniverse<'_> {
		ExclusiveUniverse::new_dangerous(self)
	}

	// === Resource Primitives === //

	pub fn init_resource<T: 'static + Send + Sync>(&self, value: T) -> &T {
		self.resources
			.add(NamedTypeId::of::<T>(), Box::new(value))
			.downcast_ref()
			.unwrap()
	}

	pub fn unload_resource<T: 'static>(&mut self) -> Option<Box<T>> {
		self.flush();
		self.resources
			.remove(&NamedTypeId::of::<T>())
			.map(|v| v.downcast().ok().unwrap())
	}

	pub fn try_resource<T: 'static>(&self) -> Option<&T> {
		self.resources
			.get(&NamedTypeId::of::<T>())
			.map(|v| v.downcast_ref::<T>().unwrap())
	}

	pub fn resource_or_init<T, F>(&self, init: F) -> &T
	where
		T: 'static + Send + Sync,
		F: FnOnce() -> T,
	{
		self.resources
			.get_or_create(NamedTypeId::of::<T>(), || Box::new(init()))
			.downcast_ref()
			.unwrap()
	}

	pub fn resource_or_panic<T: 'static>(&self) -> &T {
		self.try_resource::<T>().unwrap()
	}

	pub fn resource<T: BuildableResource>(&self) -> &T {
		self.resource_or_init(|| T::create(self))
	}

	// === Resource Aliases === //

	pub fn resource_rw<T: BuildableResourceRw>(&self) -> &RwLock<T> {
		self.resource()
	}

	pub fn resource_ref<T: BuildableResourceRw>(&self) -> RwLockReadGuard<T> {
		self.resource_rw().try_read().unwrap()
	}

	pub fn resource_mut<T: BuildableResourceRw>(&self) -> RwLockWriteGuard<T> {
		self.resource_rw().try_write().unwrap()
	}

	pub fn storage<T: 'static + Send + Sync>(&self) -> RwLockReadGuard<Storage<T>> {
		self.resource_ref()
	}

	pub fn storage_mut<T: 'static + Send + Sync>(&self) -> RwLockWriteGuard<Storage<T>> {
		self.resource_mut()
	}

	pub fn comp<T: 'static + Send + Sync>(&self, target: Entity) -> MappedRwLockReadGuard<T> {
		RwLockReadGuard::map(self.storage(), |storage| &storage[target])
	}

	pub fn comp_mut<T: 'static + Send + Sync>(&self, target: Entity) -> MappedRwLockWriteGuard<T> {
		RwLockWriteGuard::map(self.storage_mut(), |storage| &mut storage[target])
	}

	// === Archetype Primitives === //

	pub fn register_archetype<M: ?Sized>(&self, archetype: Archetype) -> ArchetypeHandle<M> {
		let id = archetype.id();
		self.archetypes.add(
			id,
			Box::new(ManagedArchetype {
				archetype: Mutex::new(archetype),
				removal_tasks: Mutex::new(Vec::new()),
			}),
		);

		ArchetypeHandle {
			_ty: PhantomData,
			id,
			universe: self.proxy(),
		}
	}

	pub fn create_archetype<M: ?Sized>(&self, name: impl DebugLabel) -> ArchetypeHandle<M> {
		self.register_archetype(Archetype::new(name))
	}

	pub fn try_archetype_by_id(&self, id: ArchetypeId) -> Option<MutexGuard<Archetype>> {
		if id.is_condemned() {
			log::error!("Attempted to acquire a dead archetype with ID {id:?} from the universe.");
			// (fallthrough)
		}

		self.archetypes
			.get(&id)
			.map(|managed| managed.archetype.try_lock().unwrap())
	}

	pub fn archetype_by_id(&self, id: ArchetypeId) -> MutexGuard<Archetype> {
		self.try_archetype_by_id(id).unwrap()
	}

	pub fn add_archetype_removal_handler(&self, id: ArchetypeId, handler: UniverseArchRemovalTask) {
		self.archetypes[&id].removal_tasks.lock().push(handler);
	}

	pub fn remove_archetype(&mut self, id: ArchetypeId) -> Archetype {
		let mut managed = self.archetypes.remove(&id).unwrap();

		for task in managed.removal_tasks.into_inner() {
			task(self, managed.archetype.get_mut());
		}

		managed.archetype.into_inner()
	}

	// === Archetype Meta === //

	// TODO

	// === Archetype Aliases === //

	pub fn archetype_handle<M: ?Sized + BuildableArchetype>(&self) -> &ArchetypeHandle<M> {
		self.resource()
	}

	pub fn archetype<M: ?Sized + BuildableArchetype>(&self) -> MappedMutexGuard<Archetype<M>> {
		MutexGuard::map(
			self.archetype_by_id(self.archetype_handle::<M>().id()),
			|arch| arch.cast_marker_mut(),
		)
	}

	// === Exclusive Helpers === //

	pub fn spawn_bundle<B: BuildableArchetype + Bundle>(
		&mut self,
		name: impl DebugLabel,
		bundle: B,
	) -> Entity {
		self.as_exclusive().spawn_bundle(name, bundle)
	}

	pub fn despawn_bundle<B: BuildableArchetype + Bundle>(&mut self, target: Entity) -> B {
		self.as_exclusive().despawn_bundle(target)
	}

	// === Flushing === //

	pub fn add_flush_task(&self, task: UniverseFlushTask) {
		self.proxied.flush_tasks.lock().push(task);
	}

	pub fn proxy(&self) -> UniverseProxy {
		UniverseProxy(Arc::downgrade(&self.proxied))
	}

	pub fn flush(&mut self) {
		// Flush maps
		self.resources.flush();
		self.archetypes.flush();

		// Process handlers
		let task_list = mem::take(&mut *self.proxied.flush_tasks.lock());
		for handler in task_list {
			handler(self);
		}
	}
}

pub trait BuildableResource: 'static + Sized + Send + Sync {
	fn create(universe: &Universe) -> Self;
}

pub trait BuildableResourceRw: 'static + Sized + Send + Sync {
	fn create(universe: &Universe) -> Self;
}

pub trait BuildableArchetype: 'static {
	fn create(universe: &Universe) -> ArchetypeHandle<Self> {
		universe.create_archetype(type_name::<Self>())
	}
}

impl<T: BuildableResourceRw> BuildableResource for RwLock<T> {
	fn create(universe: &Universe) -> Self {
		RwLock::new(T::create(universe))
	}
}

impl<M: ?Sized + BuildableArchetype> BuildableResource for ArchetypeHandle<M> {
	fn create(universe: &Universe) -> Self {
		M::create(universe)
	}
}

impl<T: 'static + Send + Sync> BuildableResourceRw for Storage<T> {
	fn create(_universe: &Universe) -> Self {
		Storage::new()
	}
}

func! {
	pub fn UniverseFlushTask(cx: &mut Universe)
}

func! {
	pub fn UniverseArchRemovalTask(cx: &mut Universe, arch: &mut Archetype)
}

// === UniverseProxy === //

#[derive(Debug, Clone)]
pub struct UniverseProxy(Weak<ProxyState>);

impl UniverseProxy {
	pub fn add_flush_task(&self, task: UniverseFlushTask) {
		let Some(proxy_state) = Weak::upgrade(&self.0) else {
			log::error!("Attempted to call `add_flush_task` on a `UniverseProxy` belonging to a dead universe.");
			return;
		};

		proxy_state.flush_tasks.lock().push(task);
	}
}

// === ArchetypeHandle === //

#[derive(Debug, Clone)]
#[repr(C)]
pub struct ArchetypeHandle<M: ?Sized = ()> {
	_ty: PhantomData<fn(M) -> M>,
	universe: UniverseProxy,
	id: ArchetypeId,
}

impl<M: ?Sized> ArchetypeHandle<M> {
	pub fn cast_marker<N: ?Sized>(self) -> ArchetypeHandle<N> {
		unsafe {
			// Safety: This struct is `repr(C)` and `N` is only ever used in a `PhantomData`.
			transmute(self)
		}
	}

	pub fn cast_marker_ref<N: ?Sized>(&self) -> &ArchetypeHandle<N> {
		unsafe {
			// Safety: This struct is `repr(C)` and `N` is only ever used in a `PhantomData`.
			transmute(self)
		}
	}

	pub fn cast_marker_mut<N: ?Sized>(&mut self) -> &mut ArchetypeHandle<N> {
		unsafe {
			// Safety: This struct is `repr(C)` and `N` is only ever used in a `PhantomData`.
			transmute(self)
		}
	}

	pub fn universe(&self) -> &UniverseProxy {
		&self.universe
	}

	pub fn id(&self) -> ArchetypeId {
		self.id
	}
}

impl<M: ?Sized> Drop for ArchetypeHandle<M> {
	fn drop(&mut self) {
		let id = self.id;

		self.universe
			.add_flush_task(UniverseFlushTask::new(move |cx| {
				cx.remove_archetype(id);
			}));
	}
}

// === ExclusiveUniverse === //

#[derive(Debug)]
pub struct ExclusiveUniverse<'r> {
	universe: &'r Universe,
}

impl<'r> ExclusiveUniverse<'r> {
	// === Conversions === //

	pub fn new(universe: &'r mut Universe) -> Self {
		Self { universe }
	}

	pub fn new_dangerous(universe: &'r Universe) -> Self {
		Self { universe }
	}

	pub fn universe_dangerous(&self) -> &'r Universe {
		self.universe
	}

	pub fn dangerous_clone(&self) -> Self {
		Self::new_dangerous(self.universe_dangerous())
	}

	pub fn into_universe_ref(self) -> &'r Universe {
		self.universe
	}

	// === Exclusive helpers === //

	pub fn spawn_bundle<B: BuildableArchetype + Bundle>(
		&mut self,
		name: impl DebugLabel,
		bundle: B,
	) -> Entity {
		self.universe_dangerous()
			.archetype::<B>()
			.spawn_with_auto_cx(self, name, bundle)
	}

	pub fn despawn_bundle<B: BuildableArchetype + Bundle>(&mut self, target: Entity) -> B {
		self.universe_dangerous()
			.archetype::<B>()
			.despawn_and_extract_auto_cx(self, target)
	}

	// === Bypasses === //

	pub fn bypass_try_resource<T>(&self) -> Option<&'r T>
	where
		T: 'static + BypassExclusivity,
	{
		self.universe_dangerous().try_resource()
	}

	pub fn bypass_resource_or_init<T, F>(&self, init: F) -> &'r T
	where
		T: 'static + Send + Sync + BypassExclusivity,
		F: FnOnce() -> T,
	{
		self.universe_dangerous().resource_or_init(init)
	}

	pub fn bypass_resource_or_panic<T>(&self) -> &'r T
	where
		T: 'static + BypassExclusivity,
	{
		self.universe_dangerous().resource_or_panic()
	}

	pub fn bypass_resource<T>(&self) -> &'r T
	where
		T: BuildableResource + BypassExclusivity,
	{
		self.universe_dangerous().resource()
	}

	pub fn bypass_resource_rw<T>(&self) -> &'r RwLock<T>
	where
		T: BuildableResourceRw + BypassExclusivity,
	{
		self.universe_dangerous().resource_rw()
	}

	pub fn bypass_resource_ref<T>(&self) -> RwLockReadGuard<'r, T>
	where
		T: BuildableResourceRw + BypassExclusivity,
	{
		self.universe_dangerous().resource_ref()
	}

	pub fn bypass_resource_mut<T>(&self) -> RwLockWriteGuard<'r, T>
	where
		T: BuildableResourceRw + BypassExclusivity,
	{
		self.universe_dangerous().resource_mut()
	}

	pub fn bypass_storage<T>(&self) -> RwLockReadGuard<'r, Storage<T>>
	where
		T: 'static + Send + Sync + BypassExclusivity,
	{
		self.universe_dangerous().storage()
	}

	pub fn bypass_storage_mut<T>(&self) -> RwLockWriteGuard<'r, Storage<T>>
	where
		T: 'static + Send + Sync + BypassExclusivity,
	{
		self.universe_dangerous().storage_mut()
	}

	pub fn bypass_comp<T>(&self, target: Entity) -> MappedRwLockReadGuard<'r, T>
	where
		T: 'static + Send + Sync + BypassExclusivity,
	{
		self.universe_dangerous().comp(target)
	}

	pub fn bypass_comp_mut<T>(&self, target: Entity) -> MappedRwLockWriteGuard<'r, T>
	where
		T: 'static + Send + Sync + BypassExclusivity,
	{
		self.universe_dangerous().comp_mut(target)
	}

	pub fn archetype_handle<M: ?Sized + BuildableArchetype>(&self) -> &'r ArchetypeHandle<M> {
		self.universe_dangerous().resource()
	}

	pub fn bypass_archetype<M>(&self) -> MappedMutexGuard<'r, Archetype<M>>
	where
		M: ?Sized + BuildableArchetype + BypassExclusivity,
	{
		self.universe_dangerous().archetype()
	}
}

impl<'r> Deref for ExclusiveUniverse<'r> {
	type Target = Universe;

	fn deref(&self) -> &Self::Target {
		self.universe
	}
}

// === BypassExclusivity === //

pub trait BypassExclusivity {}

impl<T: ?Sized + BypassExclusivity> BypassExclusivity for RwLock<T> {}

impl<T: BypassExclusivity> BypassExclusivity for Storage<T> {}

impl<T: ?Sized + BypassExclusivity> BypassExclusivity for Archetype<T> {}
