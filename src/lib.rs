#![allow(clippy::type_complexity)]

pub mod bundle;
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
		bundle::{bundle, Bundle},
		compost::{decompose, Context},
		debug::{label::NO_LABEL, lifetime::Dependent},
		entity::{Archetype, ArchetypeId, Entity},
		event::{func, injectors, DestroyQueue, EntityDestroyEvent, EventQueue, EventQueueIter},
		query::Query,
		storage::Storage,
		universe::{BypassExclusivity, ExclusiveUniverse, Universe},
	};
}

pub use prelude::*;
