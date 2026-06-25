use common_game::components::resource::{BasicResource, BasicResourceType, ComplexResource, ComplexResourceType, GenericResource, ResourceType};
use std::collections::HashMap;

#[derive(Debug, Default)]
pub(crate) struct Bag {
    basic_resources: Vec<BasicResource>,
    complex_resources: Vec<ComplexResource>,

    // The running tallies
    basic_counts: HashMap<BasicResourceType, usize>,
    complex_counts: HashMap<ComplexResourceType, usize>,
}

impl Bag {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            basic_resources: Vec::new(),
            complex_resources: Vec::new(),
            basic_counts: HashMap::new(),
            complex_counts: HashMap::new(),
        }
    }

    // --- STORAGE METHODS ---

    pub(crate) fn add_basic(&mut self, resource: BasicResource) {
        // Update the tally before storing
        *self.basic_counts.entry(resource.get_type()).or_insert(0) += 1;
        self.basic_resources.push(resource);
    }

    pub(crate) fn add_complex(&mut self, resource: ComplexResource) {
        // Update the tally before storing
        *self.complex_counts.entry(resource.get_type()).or_insert(0) += 1;
        self.complex_resources.push(resource);
    }

    /// Convenience method to store a `GenericResource` by unpacking it into the correct collection.
    pub(crate) fn add_generic(&mut self, resource: GenericResource) {
        match resource {
            GenericResource::BasicResources(basic) => self.add_basic(basic),
            GenericResource::ComplexResources(complex) => self.add_complex(complex),
        }
    }

    // --- EXTRACTION METHODS ---

    pub(crate) fn take_basic(&mut self, resource_type: BasicResourceType) -> Option<BasicResource> {
        let index = self
            .basic_resources
            .iter()
            .position(|r| r.get_type() == resource_type)?;

        let resource = self.basic_resources.remove(index);

        // Decrement the running tally safely
        if let Some(count) = self.basic_counts.get_mut(&resource_type) {
            *count = count.saturating_sub(1);
        }

        Some(resource)
    }

    pub(crate) fn take_complex(&mut self, resource_type: ComplexResourceType) -> Option<ComplexResource> {
        let index = self
            .complex_resources
            .iter()
            .position(|r| r.get_type() == resource_type)?;

        let resource = self.complex_resources.remove(index);

        // Decrement the running tally safely
        if let Some(count) = self.complex_counts.get_mut(&resource_type) {
            *count = count.saturating_sub(1);
        }

        Some(resource)
    }

    /// Convenience method to take out any resource using the unified `ResourceType`.
    pub(crate) fn take_generic(&mut self, resource_type: ResourceType) -> Option<GenericResource> {
        match resource_type {
            ResourceType::Basic(b_type) => self.take_basic(b_type).map(GenericResource::BasicResources),
            ResourceType::Complex(c_type) => self.take_complex(c_type).map(GenericResource::ComplexResources),
        }
    }

    // --- UTILITY METHODS ---

    /// Checks if the bag contains at least one basic resource of the specified type.
    pub(crate) fn contains_basic(&self, resource_type: BasicResourceType) -> bool {
        self.basic_counts.get(&resource_type).copied().unwrap_or(0) > 0
    }

    /// Checks if the bag contains at least one complex resource of the specified type.
    pub(crate) fn contains_complex(&self, resource_type: ComplexResourceType) -> bool {
        self.complex_counts.get(&resource_type).copied().unwrap_or(0) > 0
    }

    // --- SNAPSHOT ---

    /// Generates an ownership-free snapshot instantly by cloning the running tallies.
    #[must_use]
    pub(crate) fn snapshot(&self) -> BagSnapshot {
        BagSnapshot {
            // We just clone the pre-calculated HashMaps. Much faster!
            basic_resources: self.basic_counts.clone(),
            complex_resources: self.complex_counts.clone(),
        }
    }
}

/// A lightweight, clonable summary of the bag's contents.
/// Ideal for passing state to the Orchestrator without memory or borrowing limitations.
#[derive(Debug, Default, Clone)]
pub struct BagSnapshot {
    pub basic_resources: HashMap<BasicResourceType, usize>,
    pub complex_resources: HashMap<ComplexResourceType, usize>,
}

impl BagSnapshot {
    /// Convenience method for the Orchestrator to easily check the count of a basic resource.
    pub fn get_basic_count(&self, resource_type: BasicResourceType) -> usize {
        self.basic_resources.get(&resource_type).copied().unwrap_or(0)
    }

    /// Convenience method for the Orchestrator to easily check the count of a complex resource.
    pub fn get_complex_count(&self, resource_type: ComplexResourceType) -> usize {
        self.complex_resources.get(&resource_type).copied().unwrap_or(0)
    }
}