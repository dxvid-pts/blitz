use crate::{BaseDocument, Node as BlitzDomNode, local_name};
use accesskit::{Node as AccessKitNode, NodeId, Role, Tree, TreeId, TreeUpdate};

impl BaseDocument {
    pub fn build_accessibility_tree(&self) -> TreeUpdate {
        let mut nodes = std::collections::HashMap::new();
        let mut window = AccessKitNode::new(Role::Window);

        self.visit(|node_id, node| {
            let parent = node
                .parent
                .and_then(|parent_id| nodes.get_mut(&parent_id))
                .map(|(_, parent)| parent)
                .unwrap_or(&mut window);
            let (id, builder) = self.build_accessibility_node(node, parent);

            nodes.insert(node_id, (id, builder));
        });

        let mut nodes: Vec<_> = nodes
            .into_iter()
            .map(|(_, (id, node))| (id, node))
            .collect();
        nodes.push((NodeId(u64::MAX), window));

        let tree = Tree::new(NodeId(u64::MAX));
        TreeUpdate {
            tree_id: TreeId::ROOT,
            nodes,
            tree: Some(tree),
            focus: NodeId(self.focus_node_id.map(|id| id as u64).unwrap_or(u64::MAX)),
        }
    }

    fn build_accessibility_node(
        &self,
        node: &BlitzDomNode,
        parent: &mut AccessKitNode,
    ) -> (NodeId, AccessKitNode) {
        let id = NodeId(node.id as u64);

        let mut builder = AccessKitNode::default();
        if node.id == 0 {
            builder.set_role(Role::Window)
        } else if let Some(element_data) = node.element_data() {
            let name = element_data.name.local.to_string();

            // TODO match more roles
            let role = match &*name {
                "button" => Role::Button,
                "div" => Role::GenericContainer,
                "header" => Role::Header,
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => Role::Heading,
                "p" => Role::Paragraph,
                "section" => Role::Section,
                "option" => Role::ListBoxOption,
                "select" => element_data
                    .select_data()
                    .map(|select| match select.mode {
                        crate::node::SelectMode::Dropdown => Role::ComboBox,
                        crate::node::SelectMode::Listbox => Role::ListBox,
                    })
                    .unwrap_or_else(|| {
                        if element_data.attr(local_name!("multiple")).is_some()
                            || element_data
                                .attr(local_name!("size"))
                                .and_then(|value| value.parse::<usize>().ok())
                                .is_some_and(|size| size > 1)
                        {
                            Role::ListBox
                        } else {
                            Role::ComboBox
                        }
                    }),
                "input" => {
                    let ty = element_data.attr(local_name!("type")).unwrap_or("text");
                    match ty {
                        "number" => Role::NumberInput,
                        "checkbox" => Role::CheckBox,
                        _ => Role::TextInput,
                    }
                }
                _ => Role::Unknown,
            };

            builder.set_role(role);
            builder.set_html_tag(name);

            if node.element_state.contains(style_dom::ElementState::DISABLED) {
                builder.set_disabled();
            }

            if let Some(select) = element_data.select_data() {
                if let Some(active_index) = select.active_index
                    && let Some(option) = select.options.get(active_index)
                {
                    builder.set_active_descendant(NodeId(option.node_id as u64));
                }

                if matches!(select.mode, crate::node::SelectMode::Dropdown) && select.open {
                    builder.set_expanded(true);
                }

                if let Some(selected_option) = select.options.iter().find(|option| {
                    self.get_node(option.node_id)
                        .and_then(|node| node.element_data())
                        .is_some_and(|element| element.option_selected().unwrap_or(false))
                }) {
                    builder.set_value(selected_option.label.clone());
                }
            }

            if let Some(option_data) = element_data.option_data() {
                if option_data.selected {
                    builder.set_selected(true);
                }
            }
        } else if node.is_text_node() {
            builder.set_role(Role::TextRun);
            builder.set_value(node.text_content());
            parent.push_labelled_by(id)
        }

        parent.push_child(id);

        (id, builder)
    }
}
