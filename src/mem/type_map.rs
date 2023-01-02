use std::any::{type_name, Any};

use crate::debug::type_id::NamedTypeId;

use super::eventual_map::EventualMap;

#[derive(Debug, Default)]
pub struct TypeMap {
    map: EventualMap<NamedTypeId, dyn Any + Send + Sync>,
}

impl TypeMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_or_create<T, F>(&self, f: F) -> &T
    where
        T: 'static + Send + Sync,
        F: FnOnce() -> T,
    {
        self.map
            .get_or_create(NamedTypeId::of::<T>(), || Box::new(f()))
            .downcast_ref()
            .unwrap()
    }

    pub fn add<T: 'static + Send + Sync>(&self, value: T) -> &T {
        self.map
            .add(NamedTypeId::of::<T>(), Box::new(value))
            .downcast_ref()
            .unwrap()
    }

    pub fn insert<T: 'static + Send + Sync>(&mut self, value: T) -> &mut T {
        self.map
            .insert(NamedTypeId::of::<T>(), Box::new(value))
            .downcast_mut()
            .unwrap()
    }

    pub fn try_get<T: 'static + Send + Sync>(&self) -> Option<&T> {
        self.map
            .get(&NamedTypeId::of::<T>())
            .map(|v| v.downcast_ref().unwrap())
    }

    pub fn get<T: 'static + Send + Sync>(&self) -> &T {
        self.try_get().unwrap_or_else(|| {
            panic!(
                "Failed to get component of type {:?} in `TypeMap`.",
                type_name::<T>()
            )
        })
    }

    pub fn remove<T: 'static + Send + Sync>(&mut self) -> Option<Box<T>> {
        self.map
            .remove(&NamedTypeId::of::<T>())
            .map(|b| Box::<dyn Any + Send + Sync>::downcast(b).unwrap())
    }

    pub fn flush(&mut self) {
        self.map.flush();
    }
}
