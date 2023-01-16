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
		context::{decompose, provider_from_tuple, unpack, Context, Provider},
		debug::{lifetime::Dependent, label::NO_LABEL},
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
