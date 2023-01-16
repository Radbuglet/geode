#![allow(clippy::type_complexity)]

pub mod bundle;
pub mod context;
pub mod debug;
pub mod entity;
pub mod event;
pub mod query;
pub mod storage;
pub mod universe;
mod util;

pub mod prelude {
	pub use crate::{
		bundle::{bundle, Bundle},
		context::{decompose, unpack, Context, ExclusiveProvider, Provider},
		debug::{label::NO_LABEL, lifetime::Dependent},
		entity::{Archetype, ArchetypeId, Entity},
		event::{DestroyQueue, EntityDestroyEvent, EventHandler, EventQueue, EventQueueIter},
		query::Query,
		storage::Storage,
		universe::{
			ArchetypeHandle, BuildableArchetypeBundle, BuildableResource, BuildableResourceRw,
			TagHandle, TagId, Universe,
		},
	};
}

pub use prelude::*;
