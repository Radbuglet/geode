use std::{
	any::type_name,
	borrow::Borrow,
	collections::HashSet,
	fmt,
	marker::PhantomData,
	mem::{self, transmute},
	num::NonZeroU64,
	sync::{
		atomic::{AtomicU64, Ordering},
		Arc, Weak,
	},
};

use derive_where::derive_where;
use fnv::FnvBuildHasher;
use parking_lot::{MappedMutexGuard, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::{
	context::{ProviderEntries, SpawnSubProvider},
	debug::{
		label::{DebugLabel, ReifiedDebugLabel},
		lifetime::{DebugLifetime, LifetimeLike, OwnedLifetime},
	},
	entity::hashers::ArchetypeBuildHasher,
	event::{TaskQueue, UniverseEventHandler},
	util::{eventual_map::EventualMap, ptr::PointeeCastExt, type_map::TypeMap},
	Archetype, ArchetypeId, EventQueue, EventQueueIter, Provider, Storage,
};

// === Universe === //

#[derive(Debug, Default)]
pub struct Universe {
	archetypes: EventualMap<ArchetypeId, ArchetypeInner, ArchetypeBuildHasher>,
	tags: EventualMap<TagId, TagInner, FnvBuildHasher>,
	tag_alloc: AtomicU64,
	dirty_archetypes: Mutex<HashSet<ArchetypeId>>,
	resources: TypeMap,
	task_queue: Mutex<TaskQueue<UniverseTask>>,
	destruction_list: Arc<DestructionList>,
}

#[derive(Debug)]
struct ArchetypeInner {
	archetype: Mutex<Archetype>,
	meta: TypeMap,
	tags: Mutex<HashSet<TagId>>,
}

#[derive(Debug)]
struct TagInner {
	_lifetime: OwnedLifetime<DebugLifetime>,
	tagged: Mutex<HashSet<ArchetypeId>>,
}

#[derive(Debug, Default)]
struct DestructionList {
	archetypes: Mutex<Vec<ArchetypeId>>,
	tags: Mutex<Vec<TagId>>,
}

struct UniverseTask {
	name: ReifiedDebugLabel,
	handler: Box<dyn FnMut(&mut Provider) + Send + Sync>,
}

impl fmt::Debug for UniverseTask {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("UniverseTask")
			.field("name", &self.name)
			.finish_non_exhaustive()
	}
}

impl Universe {
	pub fn new() -> Self {
		Self::default()
	}

	// === Archetype Management === //

	pub fn create_archetype<M: ?Sized>(&self, name: impl DebugLabel) -> ArchetypeHandle<M> {
		let archetype = Archetype::new(name);
		let id = archetype.id();
		self.archetypes.add(
			id,
			Box::new(ArchetypeInner {
				archetype: Mutex::new(archetype),
				meta: TypeMap::default(),
				tags: Mutex::default(),
			}),
		);

		ArchetypeHandle {
			_ty: PhantomData,
			id,
			destruction_list: Arc::downgrade(&self.destruction_list),
		}
	}

	pub fn archetype_by_id(&self, id: ArchetypeId) -> &Mutex<Archetype> {
		&self.archetypes[&id].archetype
	}

	pub fn archetype_by_handle<M: ?Sized>(
		&self,
		handle: &ArchetypeHandle<M>,
	) -> MappedMutexGuard<Archetype<M>> {
		MutexGuard::map(
			self.archetype_by_id(handle.id()).try_lock().unwrap(),
			|arch| arch.cast_marker_mut(),
		)
	}

	pub fn archetype_by_handle_blocking<M: ?Sized>(
		&self,
		handle: &ArchetypeHandle<M>,
	) -> MappedMutexGuard<Archetype<M>> {
		MutexGuard::map(self.archetype_by_id(handle.id()).lock(), |arch| {
			arch.cast_marker_mut()
		})
	}

	pub fn add_archetype_meta<T: 'static + Send + Sync>(&self, id: ArchetypeId, value: T) {
		self.archetypes[&id].meta.add(value);
		self.dirty_archetypes.lock().insert(id);
	}

	pub fn add_archetype_queue_handler<E, F>(&self, id: ArchetypeId, handler: F)
	where
		E: 'static,
		F: 'static + Send + Sync,
		F: Fn(&Provider, EventQueueIter<E>) + Clone,
	{
		self.add_archetype_meta::<ArchetypeEventQueueHandler<E>>(
			id,
			ArchetypeEventQueueHandler(UniverseEventHandler::new(handler)),
		);
	}

	pub fn try_get_archetype_meta<T: 'static + Send + Sync>(&self, id: ArchetypeId) -> Option<&T> {
		self.archetypes[&id].meta.try_get()
	}

	pub fn archetype_meta<T: 'static + Send + Sync>(&self, id: ArchetypeId) -> &T {
		self.archetypes[&id].meta.get()
	}

	// === Archetype Tagging === //

	pub fn create_tag(&self, name: impl DebugLabel) -> TagHandle {
		let id = NonZeroU64::new(self.tag_alloc.fetch_add(1, Ordering::Relaxed) + 1).unwrap();
		let lifetime = DebugLifetime::new(name);
		let id = TagId { lifetime, id };

		self.tags.add(
			id,
			Box::new(TagInner {
				_lifetime: OwnedLifetime::new(lifetime),
				tagged: Mutex::default(),
			}),
		);

		TagHandle {
			id,
			destruction_list: Arc::downgrade(&self.destruction_list),
		}
	}

	pub fn tag_archetype(&self, arch: ArchetypeId, tag: TagId) {
		let did_insert = self.tags[&tag].tagged.lock().insert(arch);

		if did_insert {
			self.archetypes[&arch].tags.lock().insert(tag);
		}
	}

	pub fn tagged_archetypes(&self, tag: TagId) -> HashSet<ArchetypeId> {
		self.tags[&tag].tagged.lock().iter().copied().collect()
	}

	// === Resource Management === //

	pub fn resource<T: BuildableResource>(&self) -> &T {
		self.resources.get_or_create(|| T::create(self))
	}

	pub fn resource_rw<T: BuildableResourceRw>(&self) -> &RwLock<T> {
		self.resource()
	}

	pub fn archetype_resource_id<T: ?Sized + BuildableArchetypeBundle>(&self) -> ArchetypeId {
		self.resource::<ArchetypeHandleResource<T>>().id()
	}

	pub fn archetype<T: ?Sized + BuildableArchetypeBundle>(
		&self,
	) -> MappedMutexGuard<Archetype<T>> {
		let id = self.archetype_resource_id::<T>();
		MutexGuard::map(self.archetype_by_id(id).try_lock().unwrap(), |arch| {
			arch.cast_marker_mut()
		})
	}

	pub fn archetype_blocking<T: ?Sized + BuildableArchetypeBundle>(
		&self,
	) -> MappedMutexGuard<Archetype<T>> {
		let id = self.archetype_resource_id::<T>();
		MutexGuard::map(self.archetype_by_id(id).lock(), |arch| {
			arch.cast_marker_mut()
		})
	}

	pub fn storage<T: 'static + Send + Sync>(&self) -> RwLockReadGuard<Storage<T>> {
		self.resource_rw().try_read().unwrap()
	}

	pub fn storage_mut<T: 'static + Send + Sync>(&self) -> RwLockWriteGuard<Storage<T>> {
		self.resource_rw().try_write().unwrap()
	}

	// === Event Queue === //

	pub fn queue_task<F>(&self, name: impl DebugLabel, handler: F)
	where
		F: 'static + Send + Sync + FnOnce(&mut Provider),
	{
		let mut handler = Some(handler);

		self.task_queue.lock().push(UniverseTask {
			name: name.reify(),
			handler: Box::new(move |universe| (handler.take().unwrap())(universe)),
		});
	}

	pub fn queue_event_dispatch<E: 'static + Send + Sync>(&self, mut events: EventQueue<E>) {
		self.queue_task(
			format_args!("EventQueue<{}> dispatch", type_name::<E>()),
			move |cx| {
				let universe = cx.get_frozen::<Universe>();

				for iter in events.flush_all() {
					let arch = iter.arch();
					let handler = universe.archetype_meta::<ArchetypeEventQueueHandler<E>>(arch);
					handler.0.raw.process(cx, iter);
				}
			},
		);
	}

	// === Management === //

	pub fn sub_provider(&self) -> Provider<'_> {
		Provider::new_with(self)
	}

	pub fn sub_provider_with<'c, T: ProviderEntries<'c>>(&'c self, values: T) -> Provider<'c> {
		self.sub_provider().with(values)
	}

	pub fn dispatch_tasks(&mut self) {
		self.dispatch_tasks_with(None);
	}

	pub fn dispatch_tasks_with(&mut self, cx: Option<&Provider>) {
		while let Some(mut task) = self.task_queue.get_mut().next_task() {
			log::trace!("Executing universe task {:?}", task.name);
			(task.handler)(&mut Provider::new_inherit_with(cx, &mut *self));
		}

		self.task_queue.get_mut().clear_capacities();
	}

	pub fn flush_nurseries(&mut self) {
		// Flush all `EventualMaps`
		self.archetypes.flush();
		self.tags.flush();
		self.resources.flush();

		for archetype in mem::take(self.dirty_archetypes.get_mut()) {
			self.archetypes[&archetype].meta.flush();
		}
	}

	pub fn flush(&mut self) {
		// Flush nurseries. This is done here to ensure that the `dirty_archetypes` list is empty.
		self.flush_nurseries();

		// Flush archetype deletions
		for arch in mem::take(&mut *self.destruction_list.archetypes.lock()) {
			// Remove archetype from archetype map
			let arch_info = self.archetypes.remove(&arch).unwrap();

			// Unregister tag dependencies
			for tag in arch_info.tags.into_inner() {
				self.tags[&tag].tagged.get_mut().remove(&arch);
			}

			// (archetype is destroyed on drop)
		}

		// Flush tag deletions
		for tag in mem::take(&mut *self.destruction_list.tags.lock()) {
			// Remove tag from tag map
			let tag_info = self.tags.remove(&tag).unwrap();

			// Unregister archetypal dependencies
			for arch in tag_info.tagged.into_inner() {
				self.archetypes[&arch].tags.get_mut().remove(&tag);
			}

			// (lifetime is killed implicitly on drop)
		}
	}
}

impl SpawnSubProvider for Universe {
	fn sub_provider<'c>(&'c self) -> Provider<'c> {
		// Name resolution prioritizes inherent method of the same name.
		Provider::new_with(self)
	}

	fn sub_provider_with<'c, T: ProviderEntries<'c>>(&'c self, entries: T) -> Provider<'c> {
		// Name resolution prioritizes inherent method of the same name.
		self.sub_provider_with(entries)
	}
}

// === Handles === //

#[derive_where(Debug)]
#[repr(C)]
pub struct ArchetypeHandle<M: ?Sized = ()> {
	_ty: PhantomData<fn(M) -> M>,
	id: ArchetypeId,
	destruction_list: Weak<DestructionList>,
}

impl<M: ?Sized> ArchetypeHandle<M> {
	pub fn id(&self) -> ArchetypeId {
		self.id
	}

	pub fn get<'a>(&self, universe: &'a Universe) -> MappedMutexGuard<'a, Archetype<M>> {
		universe.archetype_by_handle(self)
	}

	pub fn get_blocking<'a>(&self, universe: &'a Universe) -> MappedMutexGuard<'a, Archetype<M>> {
		universe.archetype_by_handle_blocking(self)
	}

	pub fn annotate<T: 'static + Send + Sync>(&self, universe: &Universe, value: T) {
		universe.add_archetype_meta(self.id(), value);
	}

	pub fn tag(&self, universe: &Universe, tag: TagId) {
		universe.tag_archetype(self.id(), tag)
	}

	pub fn add_queue_handler<E, F>(&self, universe: &Universe, handler: F)
	where
		E: 'static,
		F: 'static + Send + Sync,
		F: Fn(&Provider, EventQueueIter<E>) + Clone,
	{
		universe.add_archetype_queue_handler(self.id(), handler);
	}

	pub fn cast_marker<N: ?Sized>(self) -> ArchetypeHandle<N> {
		unsafe {
			// Safety: This struct is `repr(C)` and `N` is only ever used in a `PhantomData`.
			transmute(self)
		}
	}

	pub fn cast_marker_ref<N: ?Sized>(&self) -> &ArchetypeHandle<N> {
		unsafe {
			// Safety: This struct is `repr(C)` and `N` is only ever used in a `PhantomData`.
			self.transmute_pointee_ref()
		}
	}

	pub fn cast_marker_mut<N: ?Sized>(&mut self) -> &mut ArchetypeHandle<N> {
		unsafe {
			// Safety: This struct is `repr(C)` and `N` is only ever used in a `PhantomData`.
			self.transmute_pointee_mut()
		}
	}
}

impl<M: ?Sized> Borrow<ArchetypeId> for ArchetypeHandle<M> {
	fn borrow(&self) -> &ArchetypeId {
		&self.id
	}
}

impl<M: ?Sized> Drop for ArchetypeHandle<M> {
	fn drop(&mut self) {
		let Some(dtor_list) = self.destruction_list.upgrade() else {
			log::error!("Failed to destroy ArchetypeHandle for {:?}: owning universe was destroyed.", self.id);
			return;
		};

		dtor_list.archetypes.lock().push(self.id);
	}
}

#[derive(Debug)]
pub struct TagHandle {
	id: TagId,
	destruction_list: Weak<DestructionList>,
}

impl TagHandle {
	pub fn id(&self) -> TagId {
		self.id
	}

	pub fn add(&self, universe: &Universe, arch: ArchetypeId) {
		universe.tag_archetype(arch, self.id())
	}

	pub fn tagged(&self, universe: &Universe) -> HashSet<ArchetypeId> {
		universe.tagged_archetypes(self.id())
	}
}

impl Borrow<TagId> for TagHandle {
	fn borrow(&self) -> &TagId {
		&self.id
	}
}

impl Drop for TagHandle {
	fn drop(&mut self) {
		let Some(dtor_list) = self.destruction_list.upgrade() else {
			log::error!("Failed to destroy TagHandle for {:?}: owning universe was destroyed.", self.id);
			return;
		};

		dtor_list.tags.lock().push(self.id);
	}
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub struct TagId {
	lifetime: DebugLifetime,
	id: NonZeroU64,
}

impl LifetimeLike for TagId {
	fn is_possibly_alive(self) -> bool {
		self.lifetime.is_possibly_alive()
	}

	fn is_condemned(self) -> bool {
		self.lifetime.is_condemned()
	}

	fn inc_dep(self) {
		self.lifetime.inc_dep();
	}

	fn dec_dep(self) {
		self.lifetime.dec_dep();
	}
}

// === Universe helpers === //

#[derive_where(Debug, Clone)]
pub struct ArchetypeEventQueueHandler<E: 'static>(pub UniverseEventHandler<EventQueueIter<E>>);

#[derive_where(Debug)]
pub struct ArchetypeHandleResource<T: ?Sized>(pub ArchetypeHandle<T>);

impl<T: ?Sized + BuildableArchetypeBundle> BuildableResource for ArchetypeHandleResource<T> {
	fn create(universe: &Universe) -> Self {
		Self(T::create_archetype(universe))
	}
}

impl<T: ?Sized> ArchetypeHandleResource<T> {
	pub fn id(&self) -> ArchetypeId {
		self.0.id()
	}
}

// === Resource traits === //

pub trait BuildableResource: 'static + Send + Sync {
	fn create(universe: &Universe) -> Self;
}

pub trait BuildableResourceRw: 'static + Send + Sync {
	fn create(universe: &Universe) -> Self;
}

impl<T: BuildableResourceRw> BuildableResource for RwLock<T> {
	fn create(universe: &Universe) -> Self {
		RwLock::new(T::create(universe))
	}
}

pub trait BuildableArchetypeBundle: 'static {
	fn create_archetype(universe: &Universe) -> ArchetypeHandle<Self> {
		universe.create_archetype(type_name::<Self>())
	}
}

// === `Provider` dependency injection === //

pub mod injection {
	use std::{
		borrow::BorrowMut,
		cell::{Ref, RefMut},
		ops::{Deref, DerefMut},
	};

	use parking_lot::MutexGuard;

	use crate::{context::UnpackTarget, Provider};

	use super::*;

	// === Markers === //

	pub struct Res<T>(PhantomData<fn(T) -> T>);

	pub struct ResRw<T>(PhantomData<fn(T) -> T>);

	pub struct ResArch<T: ?Sized>(PhantomData<fn(T) -> T>);

	impl<'guard: 'borrow, 'borrow> UnpackTarget<'guard, 'borrow, Universe> for &'borrow Universe {
		type Guard = &'guard Universe;
		type Reference = &'borrow Universe;

		fn acquire_guard(src: &'guard Universe) -> Self::Guard {
			src
		}

		fn acquire_ref(guard: &'borrow mut Self::Guard) -> Self::Reference {
			guard
		}
	}

	impl<'guard: 'borrow, 'borrow, T: BuildableResource> UnpackTarget<'guard, 'borrow, Universe>
		for Res<&'borrow T>
	{
		type Guard = &'guard T;
		type Reference = &'borrow T;

		fn acquire_guard(src: &'guard Universe) -> Self::Guard {
			src.resource()
		}

		fn acquire_ref(guard: &'borrow mut Self::Guard) -> Self::Reference {
			guard
		}
	}

	impl<'guard: 'borrow, 'borrow, T: BuildableResourceRw> UnpackTarget<'guard, 'borrow, Universe>
		for ResRw<&'borrow T>
	{
		type Guard = RwLockReadGuard<'guard, T>;
		type Reference = &'borrow T;

		fn acquire_guard(src: &'guard Universe) -> Self::Guard {
			src.resource_rw().try_read().unwrap()
		}

		fn acquire_ref(guard: &'borrow mut Self::Guard) -> Self::Reference {
			&*guard
		}
	}

	impl<'guard: 'borrow, 'borrow, T: BuildableResourceRw> UnpackTarget<'guard, 'borrow, Universe>
		for ResRw<&'borrow mut T>
	{
		type Guard = RwLockWriteGuard<'guard, T>;
		type Reference = &'borrow mut T;

		fn acquire_guard(src: &'guard Universe) -> Self::Guard {
			src.resource_rw().try_write().unwrap()
		}

		fn acquire_ref(guard: &'borrow mut Self::Guard) -> Self::Reference {
			&mut *guard
		}
	}

	impl<'guard: 'borrow, 'borrow, T: ?Sized + BuildableArchetypeBundle>
		UnpackTarget<'guard, 'borrow, Universe> for ResArch<T>
	{
		type Guard = MappedMutexGuard<'guard, Archetype<T>>;
		type Reference = &'borrow mut Archetype<T>;

		fn acquire_guard(src: &'guard Universe) -> Self::Guard {
			let id = src.archetype_resource_id::<T>();
			MutexGuard::map(src.archetype_by_id(id).try_lock().unwrap(), |arch| {
				arch.cast_marker_mut()
			})
		}

		fn acquire_ref(guard: &'borrow mut Self::Guard) -> Self::Reference {
			guard
		}
	}

	impl<'provider, 'guard: 'borrow, 'borrow, T: BuildableResource>
		UnpackTarget<'guard, 'borrow, Provider<'provider>> for Res<&'borrow T>
	{
		type Guard = ProviderResourceGuard<'guard, T>;
		type Reference = &'borrow T;

		fn acquire_guard(src: &'guard Provider<'provider>) -> Self::Guard {
			if let Some(value) = src.try_get::<T>() {
				return ProviderResourceGuard::Local(value);
			}

			ProviderResourceGuard::Universe(src.get_frozen::<Universe>().resource::<T>())
		}

		fn acquire_ref(guard: &'borrow mut Self::Guard) -> Self::Reference {
			&*guard
		}
	}

	impl<'provider, 'guard: 'borrow, 'borrow, T: BuildableResourceRw>
		UnpackTarget<'guard, 'borrow, Provider<'provider>> for ResRw<&'borrow T>
	{
		type Guard = ProviderResourceRefGuard<'guard, T>;
		type Reference = &'borrow T;

		fn acquire_guard(src: &'guard Provider<'provider>) -> Self::Guard {
			if let Some(value) = src.try_get::<T>() {
				return ProviderResourceRefGuard::Local(value);
			}

			ProviderResourceRefGuard::Universe(
				src.get_frozen::<Universe>()
					.resource_rw()
					.try_read()
					.unwrap(),
			)
		}

		fn acquire_ref(guard: &'borrow mut Self::Guard) -> Self::Reference {
			&*guard
		}
	}

	impl<'provider, 'guard: 'borrow, 'borrow, T: BuildableResourceRw>
		UnpackTarget<'guard, 'borrow, Provider<'provider>> for ResRw<&'borrow mut T>
	{
		type Guard = ProviderResourceMutGuard<'guard, T>;
		type Reference = &'borrow mut T;

		fn acquire_guard(src: &'guard Provider<'provider>) -> Self::Guard {
			if let Some(value) = src.try_get_mut::<T>() {
				return ProviderResourceMutGuard::Local(value);
			}

			ProviderResourceMutGuard::Universe(
				src.get_frozen::<Universe>()
					.resource_rw()
					.try_write()
					.unwrap(),
			)
		}

		fn acquire_ref(guard: &'borrow mut Self::Guard) -> Self::Reference {
			&mut *guard
		}
	}

	impl<'provider, 'guard: 'borrow, 'borrow, T: ?Sized + BuildableArchetypeBundle>
		UnpackTarget<'guard, 'borrow, Provider<'provider>> for ResArch<T>
	{
		type Guard = ProviderResourceArchGuard<'guard, T>;
		type Reference = &'borrow mut Archetype<T>;

		fn acquire_guard(src: &'guard Provider<'provider>) -> Self::Guard {
			if let Some(value) = src.try_get_mut::<Archetype<T>>() {
				return ProviderResourceArchGuard::Local(value);
			}

			let universe = src.get_frozen::<Universe>();
			let arch_id = universe.archetype_resource_id::<T>();
			let arch_mutex = universe.archetype_by_id(arch_id);

			ProviderResourceArchGuard::Universe(arch_mutex.try_lock().unwrap())
		}

		fn acquire_ref(guard: &'borrow mut Self::Guard) -> Self::Reference {
			guard.cast_marker_mut()
		}
	}

	// === Guards === //

	#[derive(Debug)]
	pub enum ProviderResourceGuard<'a, T: 'static> {
		Local(Ref<'a, T>),
		Universe(&'a T),
	}

	impl<T: 'static> Deref for ProviderResourceGuard<'_, T> {
		type Target = T;

		fn deref(&self) -> &Self::Target {
			match self {
				Self::Local(r) => r,
				Self::Universe(r) => r,
			}
		}
	}

	impl<T> Borrow<T> for ProviderResourceGuard<'_, T> {
		fn borrow(&self) -> &T {
			self
		}
	}

	#[derive(Debug)]
	pub enum ProviderResourceRefGuard<'a, T: 'static> {
		Local(Ref<'a, T>),
		Universe(RwLockReadGuard<'a, T>),
	}

	impl<T: 'static> Deref for ProviderResourceRefGuard<'_, T> {
		type Target = T;

		fn deref(&self) -> &Self::Target {
			match self {
				Self::Local(r) => r,
				Self::Universe(r) => r,
			}
		}
	}

	impl<T> Borrow<T> for ProviderResourceRefGuard<'_, T> {
		fn borrow(&self) -> &T {
			self
		}
	}

	#[derive(Debug)]
	pub enum ProviderResourceMutGuard<'a, T: 'static> {
		Local(RefMut<'a, T>),
		Universe(RwLockWriteGuard<'a, T>),
	}

	impl<T: 'static> Deref for ProviderResourceMutGuard<'_, T> {
		type Target = T;

		fn deref(&self) -> &Self::Target {
			match self {
				Self::Local(r) => r,
				Self::Universe(r) => r,
			}
		}
	}

	impl<T: 'static> DerefMut for ProviderResourceMutGuard<'_, T> {
		fn deref_mut(&mut self) -> &mut Self::Target {
			match self {
				Self::Local(r) => &mut *r,
				Self::Universe(r) => &mut *r,
			}
		}
	}

	impl<T> Borrow<T> for ProviderResourceMutGuard<'_, T> {
		fn borrow(&self) -> &T {
			self
		}
	}

	impl<T> BorrowMut<T> for ProviderResourceMutGuard<'_, T> {
		fn borrow_mut(&mut self) -> &mut T {
			&mut *self
		}
	}

	#[derive(Debug)]
	pub enum ProviderResourceArchGuard<'a, T: ?Sized + 'static> {
		Local(RefMut<'a, Archetype<T>>),
		Universe(MutexGuard<'a, Archetype<()>>),
	}

	impl<T: ?Sized> Deref for ProviderResourceArchGuard<'_, T> {
		type Target = Archetype<T>;

		fn deref(&self) -> &Self::Target {
			match self {
				Self::Local(r) => r,
				Self::Universe(r) => r.cast_marker_ref(),
			}
		}
	}

	impl<T: ?Sized> DerefMut for ProviderResourceArchGuard<'_, T> {
		fn deref_mut(&mut self) -> &mut Self::Target {
			match self {
				Self::Local(r) => &mut *r,
				Self::Universe(r) => r.cast_marker_mut(),
			}
		}
	}

	impl<T: ?Sized> Borrow<Archetype<T>> for ProviderResourceArchGuard<'_, T> {
		fn borrow(&self) -> &Archetype<T> {
			self
		}
	}

	impl<T: ?Sized> BorrowMut<Archetype<T>> for ProviderResourceArchGuard<'_, T> {
		fn borrow_mut(&mut self) -> &mut Archetype<T> {
			&mut *self
		}
	}
}
