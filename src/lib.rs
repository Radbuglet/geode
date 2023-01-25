#![allow(clippy::type_complexity)]

pub mod debug;
pub mod entity;
pub mod event;
pub mod query;
pub mod storage;
pub mod universe;
mod util;

pub use {compost, parking_lot};

pub mod prelude {
	pub use crate::{
		compost::{decompose, Context},
		debug::{label::NO_LABEL, lifetime::Dependent},
		entity::{
			bundle, Archetype, ArchetypeId, ArchetypeMap, ArchetypeSet, Bundle, Entity, EntityMap,
			EntitySet, WeakArchetypeId, WeakArchetypeMap,
		},
		event::{func, injectors, DestroyQueue, EntityDestroyEvent, EventQueue, EventQueueIter},
		query::Query,
		storage::{Storage, StorageView, StorageViewMut},
		universe::{BypassExclusivity, ExclusiveUniverse, Universe},
	};
}

pub use prelude::*;
