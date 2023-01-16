#![allow(clippy::type_complexity)]

pub mod bundle;
pub mod context;
pub mod debug;
pub mod entity;
pub mod event;
pub mod query;
pub mod storage;
mod util;

pub mod prelude {
	pub use crate::{
		bundle::{bundle, Bundle},
		context::{decompose, BypassExclusivity, Context, ExclusiveUniverse, Universe},
		debug::{label::NO_LABEL, lifetime::Dependent},
		entity::{Archetype, ArchetypeId, Entity},
		event::{DestroyQueue, EntityDestroyEvent, EventQueue, EventQueueIter, OpaqueBox},
		query::Query,
		storage::Storage,
	};
}

pub use prelude::*;
