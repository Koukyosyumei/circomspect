use num_bigint::BigInt;

use crate::environment::VarEnvironment;

pub trait ValueMeta {
    /// Propagate variable values defined by the environment to each sub-node.
    fn propagate_values(&mut self, env: &VarEnvironment<ValueReduction>);

    /// Returns true if the node reduces to a constant value.
    fn is_constant(&self) -> bool;

    /// Returns true if the node reduces to a boolean value.
    fn is_boolean(&self) -> bool;

    /// Returns true if the node reduces to a field element.
    fn is_field_element(&self) -> bool;

    /// Returns the value if the node reduces to a constant, and None otherwise.
    fn get_reduces_to(&self) -> Option<&ValueReduction>;
}

#[derive(Clone)]
pub enum ValueReduction {
    Boolean { value: bool },
    FieldElement { value: BigInt },
}

// Knowledge buckets
#[derive(Default, Clone)]
pub struct ValueKnowledge {
    reduces_to: Option<ValueReduction>,
}
impl ValueKnowledge {
    pub fn new() -> ValueKnowledge {
        ValueKnowledge::default()
    }
    pub fn set_reduces_to(&mut self, reduces_to: ValueReduction) {
        self.reduces_to = Option::Some(reduces_to);
    }
    pub fn get_reduces_to(&self) -> Option<&ValueReduction> {
        self.reduces_to.as_ref()
    }
    pub fn is_constant(&self) -> bool {
        self.reduces_to.is_some()
    }
    pub fn is_boolean(&self) -> bool {
        use ValueReduction::*;
        matches!(self.reduces_to, Some(Boolean { .. }))
    }
    pub fn is_field_element(&self) -> bool {
        use ValueReduction::*;
        matches!(self.reduces_to, Some(FieldElement { .. }))
    }
}
