//! Native AccessKit accessibility adapter.
#![forbid(unsafe_code)]

pub use accesskit::{Node, NodeId, Role, TreeUpdate};

#[cfg(test)]
mod tests {
    use super::{Node, NodeId, Role};

    #[test]
    fn node_id_is_distinct() {
        let a = NodeId(1);
        let b = NodeId(2);
        assert_ne!(a, b, "distinct NodeId values must compare unequal");
        assert_eq!(a, NodeId(1), "equal NodeId values must compare equal");
    }

    #[test]
    fn node_id_from_u64() {
        let id = NodeId::from(42u64);
        assert_eq!(id, NodeId(42), "NodeId::from should wrap the given value");
    }

    #[test]
    fn role_variants_are_distinct() {
        assert_ne!(Role::Button, Role::TextInput);
        assert_ne!(Role::Button, Role::RadioButton);
        assert_ne!(Role::TextInput, Role::RadioButton);
    }

    #[test]
    fn node_new_has_given_role() {
        let node = Node::new(Role::Button);
        assert_eq!(node.role(), Role::Button);

        let text_node = Node::new(Role::TextInput);
        assert_eq!(text_node.role(), Role::TextInput);
        assert_ne!(node.role(), text_node.role());
    }
}
