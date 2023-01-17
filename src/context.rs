use std::{any::Any, ops::Deref};

use fnv::FnvBuildHasher;
use parking_lot::{Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::{
	event::TaskQueue,
	util::{eventual_map::EventualMap, type_id::NamedTypeId},
	Archetype, OpaqueBox, Storage,
};

// === Universe === //

#[derive(Debug, Default)]
pub struct Universe {
	resources: EventualMap<NamedTypeId, dyn Any + Send + Sync, FnvBuildHasher>,
	flush_tasks: Mutex<TaskQueue<OpaqueBox<dyn FnMut(&mut ExclusiveUniverse)>>>,
}

impl Universe {
	pub fn new() -> Self {
		Self::default()
	}

	pub fn as_exclusive(&mut self) -> ExclusiveUniverse<'_> {
		ExclusiveUniverse::new(self)
	}

	// === Primitive accessors === //

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

	// === Accessor aliases === //

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

	pub fn archetype<M: ?Sized + BuildableArchetypeBundle>(&self) -> RwLockReadGuard<Archetype<M>> {
		self.resource_ref()
	}

	pub fn archetype_mut<M: ?Sized + BuildableArchetypeBundle>(
		&self,
	) -> RwLockWriteGuard<Archetype<M>> {
		self.resource_mut()
	}

	// === Flushing === //

	pub fn add_flush_task(&self, task: OpaqueBox<dyn FnMut(&mut ExclusiveUniverse)>) {
		self.flush_tasks.lock().push(task);
	}

	pub fn flush(&mut self) {
		self.resources.flush();

		while let Some(mut task) = self.flush_tasks.get_mut().next_task() {
			task(&mut self.as_exclusive());
		}
	}
}

pub trait BuildableResource: 'static + Sized + Send + Sync {
	fn create(universe: &Universe) -> Self;
}

pub trait BuildableResourceRw: 'static + Sized + Send + Sync {
	fn create(universe: &Universe) -> Self;
}

pub trait BuildableArchetypeBundle: 'static {
	fn create(universe: &Universe) -> Archetype<Self>;
}

impl<T: BuildableResourceRw> BuildableResource for RwLock<T> {
	fn create(universe: &Universe) -> Self {
		RwLock::new(T::create(universe))
	}
}

impl<M: ?Sized + BuildableArchetypeBundle> BuildableResourceRw for Archetype<M> {
	fn create(universe: &Universe) -> Self {
		M::create(universe)
	}
}

impl<T: 'static + Send + Sync> BuildableResourceRw for Storage<T> {
	fn create(_universe: &Universe) -> Self {
		Storage::new()
	}
}

// === ExclusiveUniverse === //

#[derive(Debug)]
pub struct ExclusiveUniverse<'r> {
	universe: &'r Universe,
}

impl<'r> ExclusiveUniverse<'r> {
	// Conversions
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

	// Bypasses
	pub fn bypass_try_resource<T>(&self) -> Option<&T>
	where
		T: 'static + BypassExclusivity,
	{
		self.universe_dangerous().try_resource()
	}

	pub fn bypass_resource_or_init<T, F>(&self, init: F) -> &T
	where
		T: 'static + Send + Sync + BypassExclusivity,
		F: FnOnce() -> T,
	{
		self.universe_dangerous().resource_or_init(init)
	}

	pub fn bypass_resource_or_panic<T>(&self) -> &T
	where
		T: 'static + BypassExclusivity,
	{
		self.universe_dangerous().resource_or_panic()
	}

	pub fn bypass_resource<T>(&self) -> &T
	where
		T: BuildableResource + BypassExclusivity,
	{
		self.universe_dangerous().resource()
	}

	pub fn bypass_resource_rw<T>(&self) -> &RwLock<T>
	where
		T: BuildableResourceRw + BypassExclusivity,
	{
		self.universe_dangerous().resource_rw()
	}

	pub fn bypass_resource_ref<T>(&self) -> RwLockReadGuard<T>
	where
		T: BuildableResourceRw + BypassExclusivity,
	{
		self.universe_dangerous().resource_ref()
	}

	pub fn bypass_resource_mut<T>(&self) -> RwLockWriteGuard<T>
	where
		T: BuildableResourceRw + BypassExclusivity,
	{
		self.universe_dangerous().resource_mut()
	}

	pub fn bypass_storage<T>(&self) -> RwLockReadGuard<Storage<T>>
	where
		T: 'static + Send + Sync + BypassExclusivity,
	{
		self.universe_dangerous().storage()
	}

	pub fn bypass_storage_mut<T>(&self) -> RwLockWriteGuard<Storage<T>>
	where
		T: 'static + Send + Sync + BypassExclusivity,
	{
		self.universe_dangerous().storage_mut()
	}

	pub fn bypass_archetype<M>(&self) -> RwLockReadGuard<Archetype<M>>
	where
		M: ?Sized + BuildableArchetypeBundle + BypassExclusivity,
	{
		self.universe_dangerous().archetype()
	}

	pub fn bypass_archetype_mut<M>(&self) -> RwLockWriteGuard<Archetype<M>>
	where
		M: ?Sized + BuildableArchetypeBundle + BypassExclusivity,
	{
		self.universe_dangerous().resource_mut()
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

// === Compost === //

pub use compost::{decompose, Context};
