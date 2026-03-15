use crate::node::{
    OptionData, SelectData, SelectMode, SelectOption, SpecialElementData, TextBrush,
};
use crate::traversal::TreeTraverser;
use crate::{BaseDocument, ElementData, local_name, stylo_to_parley};
use blitz_traits::events::{BlitzInputEvent, DomEvent, DomEventData, HitResult};
use markup5ever::LocalName;

fn resolve_line_height(line_height: parley::LineHeight, font_size: f32) -> f32 {
    match line_height {
        parley::LineHeight::FontSizeRelative(relative) => relative * font_size,
        parley::LineHeight::Absolute(absolute) => absolute,
        parley::LineHeight::MetricsRelative(relative) => relative * font_size,
    }
}

fn truthy_attr(element: &ElementData, name: markup5ever::LocalName) -> Option<bool> {
    element
        .attr(name)
        .map(|value| value == "true" || value.is_empty())
}

fn option_is_disabled(doc: &BaseDocument, select_id: usize, option_id: usize) -> bool {
    let node = &doc.nodes[option_id];
    if node.attr(local_name!("disabled")).is_some() {
        return true;
    }

    let mut current = node.parent;
    while let Some(parent_id) = current {
        if parent_id == select_id {
            break;
        }
        let parent = &doc.nodes[parent_id];
        if parent
            .data
            .is_element_with_tag_name(&local_name!("optgroup"))
            && parent.attr(local_name!("disabled")).is_some()
        {
            return true;
        }
        current = parent.parent;
    }

    false
}

fn option_value(doc: &BaseDocument, option_id: usize) -> String {
    doc.nodes[option_id]
        .attr(local_name!("value"))
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| doc.nodes[option_id].text_content())
}

fn option_label(doc: &BaseDocument, option_id: usize) -> String {
    doc.nodes[option_id]
        .attr(local_name!("label"))
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| doc.nodes[option_id].text_content())
}

fn build_option_layout(
    doc: &mut BaseDocument,
    option_id: usize,
    label: &str,
) -> Box<parley::Layout<TextBrush>> {
    let parley_style = doc
        .nodes
        .get(option_id)
        .and_then(|node| node.primary_styles())
        .map(|styles| stylo_to_parley::style(option_id, &styles))
        .unwrap_or_default();

    let mut font_ctx = doc.font_ctx.lock().unwrap();
    let mut builder =
        doc.layout_ctx
            .tree_builder(&mut font_ctx, doc.viewport.scale(), true, &parley_style);
    builder.push_text(label);
    let mut layout = builder.build().0;
    let width = layout.calculate_content_widths().max.max(1.0);
    layout.break_all_lines(Some(width));
    Box::new(layout)
}

fn select_mode(element: &ElementData) -> SelectMode {
    let size = element
        .attr(local_name!("size"))
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    if element.attr(local_name!("multiple")).is_some() || size > 1 {
        SelectMode::Listbox
    } else {
        SelectMode::Dropdown
    }
}

impl BaseDocument {
    pub(crate) fn enclosing_select_id(&self, mut node_id: usize) -> Option<usize> {
        loop {
            let node = self.nodes.get(node_id)?;
            if node.data.is_element_with_tag_name(&local_name!("select")) {
                return Some(node_id);
            }
            node_id = node.parent?;
        }
    }

    pub(crate) fn sync_select_control(&mut self, select_id: usize) {
        let Some(select_element) = self.nodes[select_id].element_data() else {
            return;
        };

        if select_element.name.local != local_name!("select") {
            return;
        }

        let mode = select_mode(select_element);
        let visible_rows = select_element
            .attr(local_name!("size"))
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or_else(|| {
                if matches!(mode, SelectMode::Dropdown) {
                    1
                } else {
                    let option_count = TreeTraverser::new_with_root(self, select_id)
                        .filter(|id| {
                            *id != select_id
                                && self.nodes[*id]
                                    .data
                                    .is_element_with_tag_name(&local_name!("option"))
                                && self.nodes[*id].is_element()
                        })
                        .count();
                    option_count.max(1)
                }
            });

        let mut options = Vec::new();
        let mut selected_from_attr = Vec::new();
        let controlled_value = select_element
            .attr(local_name!("value"))
            .map(ToOwned::to_owned);
        let select_styles = self.nodes[select_id].primary_styles();
        let row_height = select_styles
            .as_ref()
            .map(|styles| {
                let parley_style = stylo_to_parley::style(select_id, styles);
                resolve_line_height(parley_style.line_height, parley_style.font_size) + 6.0
            })
            .unwrap_or(22.0);
        drop(select_styles);

        let previous = self.nodes[select_id]
            .element_data()
            .and_then(|element| element.select_data())
            .cloned();

        let option_ids: Vec<_> = TreeTraverser::new_with_root(self, select_id).collect();
        for option_id in option_ids {
            if option_id == select_id
                || !self.nodes[option_id]
                    .data
                    .is_element_with_tag_name(&local_name!("option"))
            {
                continue;
            }

            let existing = self.nodes[option_id]
                .element_data()
                .and_then(|element| element.option_data())
                .cloned()
                .unwrap_or_default();

            let Some(option_element) = self.nodes[option_id].element_data() else {
                continue;
            };

            let default_selected = truthy_attr(option_element, LocalName::from("initial_selected"))
                .unwrap_or(existing.default_selected);
            let selected =
                truthy_attr(option_element, local_name!("selected")).unwrap_or(existing.selected);

            if let Some(option_element) = self.nodes[option_id].element_data_mut() {
                option_element.special_data = SpecialElementData::Option(OptionData {
                    selected,
                    default_selected,
                });
            }

            if selected {
                selected_from_attr.push(option_id);
            }

            let label = option_label(self, option_id);
            let value = option_value(self, option_id);
            let disabled = option_is_disabled(self, select_id, option_id);
            let layout = build_option_layout(self, option_id, &label);
            options.push(SelectOption {
                node_id: option_id,
                value,
                label,
                disabled,
                layout,
            });
        }

        let mut selected_option_ids = if matches!(mode, SelectMode::Dropdown) {
            if let Some(controlled_value) = controlled_value {
                options
                    .iter()
                    .find(|option| option.value == controlled_value)
                    .map(|option| vec![option.node_id])
                    .unwrap_or_default()
            } else if !selected_from_attr.is_empty() {
                vec![selected_from_attr[0]]
            } else if let Some(existing_selected) = options.iter().find(|option| {
                self.nodes[option.node_id]
                    .element_data()
                    .and_then(|element| element.option_data())
                    .is_some_and(|data| data.selected)
            }) {
                vec![existing_selected.node_id]
            } else {
                options
                    .iter()
                    .find(|option| !option.disabled)
                    .map(|option| vec![option.node_id])
                    .unwrap_or_default()
            }
        } else if !selected_from_attr.is_empty() {
            selected_from_attr
        } else {
            options
                .iter()
                .filter(|option| {
                    self.nodes[option.node_id]
                        .element_data()
                        .and_then(|element| element.option_data())
                        .is_some_and(|data| data.selected)
                })
                .map(|option| option.node_id)
                .collect()
        };

        if matches!(mode, SelectMode::Dropdown) && selected_option_ids.is_empty() {
            if let Some(first_enabled) = options.iter().find(|option| !option.disabled) {
                selected_option_ids.push(first_enabled.node_id);
            }
        }

        for option in &options {
            if let Some(option_data) = self.nodes[option.node_id]
                .element_data_mut()
                .and_then(|element| element.option_data_mut())
            {
                option_data.selected = selected_option_ids.contains(&option.node_id);
            }
        }

        let previous_open = previous.as_ref().is_some_and(|data| data.open);
        let previous_active = previous.as_ref().and_then(|data| data.active_index);

        let active_index = previous_active.or_else(|| {
            options.iter().position(|option| {
                self.nodes[option.node_id]
                    .element_data()
                    .and_then(|element| element.option_data())
                    .is_some_and(|data| data.selected)
            })
        });

        if let Some(select_element) = self.nodes[select_id].element_data_mut() {
            select_element.special_data = SpecialElementData::Select(SelectData {
                mode,
                open: previous_open && matches!(mode, SelectMode::Dropdown),
                active_index,
                anchor_index: active_index,
                row_height,
                visible_rows,
                popup_rows: options.len().max(1),
                popup_scroll: 0.0,
                options,
            });
        }
    }

    pub(crate) fn close_open_select(&mut self) -> bool {
        let Some(select_id) = self.open_select_id.take() else {
            return false;
        };
        let changed = if let Some(select_data) = self.nodes[select_id]
            .element_data_mut()
            .and_then(|element| element.select_data_mut())
        {
            let was_open = select_data.open;
            select_data.open = false;
            was_open
        } else {
            false
        };
        if changed {
            self.changed_nodes.insert(select_id);
            self.shell_provider.request_redraw();
        }
        changed
    }

    pub fn open_select_popup_id(&self) -> Option<usize> {
        self.open_select_id
    }

    pub(crate) fn open_select(&mut self, select_id: usize) -> bool {
        let _ = self.close_open_select();
        let default_active = self.nodes[select_id]
            .element_data()
            .and_then(|element| element.select_data())
            .and_then(|select| {
                select.options.iter().position(|option| {
                    self.nodes[option.node_id]
                        .element_data()
                        .and_then(|element| element.option_data())
                        .is_some_and(|data| data.selected)
                })
            });
        let changed = if let Some(select_data) = self.nodes[select_id]
            .element_data_mut()
            .and_then(|element| element.select_data_mut())
        {
            if matches!(select_data.mode, SelectMode::Dropdown) {
                let was_open = select_data.open;
                select_data.open = true;
                if select_data.active_index.is_none() {
                    select_data.active_index = default_active;
                }
                !was_open
            } else {
                false
            }
        } else {
            false
        };

        if changed {
            self.open_select_id = Some(select_id);
            self.changed_nodes.insert(select_id);
            self.shell_provider.request_redraw();
        }

        changed
    }

    pub(crate) fn select_popup_hit(&self, x: f32, y: f32) -> Option<HitResult> {
        let select_id = self.open_select_id?;
        let bounds = self.select_popup_bounds(select_id)?;
        if x < bounds.x
            || x > bounds.x + bounds.width
            || y < bounds.y
            || y > bounds.y + bounds.height
        {
            return None;
        }

        let node = &self.nodes[select_id];
        let local_x = x - bounds.x;
        let local_y = node.final_layout.size.height + (y - bounds.y);
        Some(HitResult {
            node_id: select_id,
            is_text: false,
            x: local_x,
            y: local_y,
        })
    }

    pub(crate) fn select_popup_bounds(&self, select_id: usize) -> Option<SelectPopupBounds> {
        let node = &self.nodes[select_id];
        let select = node.element_data()?.select_data()?;
        if !select.open || !matches!(select.mode, SelectMode::Dropdown) {
            return None;
        }

        let pos = node.absolute_position(0.0, 0.0);
        let width = node
            .final_layout
            .size
            .width
            .max(self.select_popup_width(select_id));
        let height = select.row_height * select.popup_rows as f32;
        Some(SelectPopupBounds {
            x: pos.x,
            y: pos.y + node.final_layout.size.height,
            width,
            height,
        })
    }

    pub fn select_popup_rect(&self, select_id: usize) -> Option<(f32, f32, f32, f32)> {
        let bounds = self.select_popup_bounds(select_id)?;
        Some((bounds.x, bounds.y, bounds.width, bounds.height))
    }

    pub(crate) fn select_popup_width(&self, select_id: usize) -> f32 {
        let Some(select) = self.nodes[select_id]
            .element_data()
            .and_then(|element| element.select_data())
        else {
            return self.nodes[select_id].final_layout.size.width;
        };

        let widest = select
            .options
            .iter()
            .map(|option| option.layout.full_width() + 24.0)
            .fold(0.0f32, f32::max);
        self.nodes[select_id].final_layout.size.width.max(widest)
    }

    pub(crate) fn select_option_index_at_local_y(
        &self,
        select_id: usize,
        local_y: f32,
    ) -> Option<usize> {
        let node = &self.nodes[select_id];
        let select = node.element_data()?.select_data()?;
        let row_height = select.row_height.max(1.0);

        match select.mode {
            SelectMode::Dropdown => {
                if !select.open || local_y < node.final_layout.size.height {
                    return None;
                }
                let popup_y = local_y - node.final_layout.size.height;
                let index = (popup_y / row_height).floor() as usize;
                (index < select.options.len()).then_some(index)
            }
            SelectMode::Listbox => {
                let index = (local_y / row_height).floor() as usize;
                (index < select.options.len()).then_some(index)
            }
        }
    }

    pub(crate) fn set_select_active_from_local_y(
        &mut self,
        select_id: usize,
        local_y: f32,
    ) -> bool {
        let Some(index) = self.select_option_index_at_local_y(select_id, local_y) else {
            return false;
        };
        let changed = if let Some(select) = self.nodes[select_id]
            .element_data_mut()
            .and_then(|element| element.select_data_mut())
        {
            if select
                .options
                .get(index)
                .is_some_and(|option| !option.disabled)
            {
                let did_change = select.active_index != Some(index);
                select.active_index = Some(index);
                did_change
            } else {
                false
            }
        } else {
            false
        };
        if changed {
            self.changed_nodes.insert(select_id);
            self.shell_provider.request_redraw();
        }
        changed
    }

    pub(crate) fn select_is_multiple(&self, select_id: usize) -> bool {
        self.nodes[select_id]
            .element_data()
            .is_some_and(|element| element.attr(local_name!("multiple")).is_some())
    }

    pub(crate) fn select_active_index(&self, select_id: usize) -> Option<usize> {
        self.nodes[select_id]
            .element_data()
            .and_then(|element| element.select_data())
            .and_then(|select| select.active_index)
    }

    pub(crate) fn first_enabled_select_index(&self, select_id: usize) -> Option<usize> {
        self.nodes[select_id]
            .element_data()
            .and_then(|element| element.select_data())
            .and_then(|select| select.options.iter().position(|option| !option.disabled))
    }

    pub(crate) fn last_enabled_select_index(&self, select_id: usize) -> Option<usize> {
        self.nodes[select_id]
            .element_data()
            .and_then(|element| element.select_data())
            .and_then(|select| select.options.iter().rposition(|option| !option.disabled))
    }

    pub(crate) fn selected_option_indices(&self, select_id: usize) -> Vec<usize> {
        self.nodes[select_id]
            .element_data()
            .and_then(|element| element.select_data())
            .map(|select| {
                select
                    .options
                    .iter()
                    .enumerate()
                    .filter_map(|(index, option)| {
                        self.nodes[option.node_id]
                            .element_data()
                            .and_then(|element| element.option_data())
                            .is_some_and(|data| data.selected)
                            .then_some(index)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn primary_select_value(&self, select_id: usize) -> Option<String> {
        let select = self.nodes[select_id].element_data()?.select_data()?;
        let index = self
            .selected_option_indices(select_id)
            .into_iter()
            .next()
            .or(select.active_index)?;
        select.options.get(index).map(|option| option.value.clone())
    }

    pub(crate) fn find_select_option_by_prefix(
        &self,
        select_id: usize,
        prefix: &str,
    ) -> Option<usize> {
        let select = self.nodes[select_id].element_data()?.select_data()?;
        if prefix.is_empty() {
            return None;
        }

        let prefix = prefix.to_lowercase();
        let start = self
            .select_active_index(select_id)
            .or_else(|| self.selected_option_indices(select_id).into_iter().next())
            .map(|index| index + 1)
            .unwrap_or(0);

        (0..select.options.len())
            .map(|offset| (start + offset) % select.options.len())
            .find(|index| {
                let option = &select.options[*index];
                !option.disabled && option.label.to_lowercase().starts_with(&prefix)
            })
    }

    pub(crate) fn select_form_values(&self, select_id: usize) -> Vec<(String, String)> {
        let Some(select) = self.nodes[select_id]
            .element_data()
            .and_then(|element| element.select_data())
        else {
            return Vec::new();
        };

        select
            .options
            .iter()
            .filter(|option| !option.disabled)
            .filter_map(|option| {
                self.nodes[option.node_id]
                    .element_data()
                    .and_then(|element| element.option_data())
                    .is_some_and(|data| data.selected)
                    .then_some((option.value.clone(), option.label.clone()))
            })
            .collect()
    }

    pub(crate) fn set_select_indices<F: FnMut(DomEvent)>(
        &mut self,
        select_id: usize,
        indices: &[usize],
        mut dispatch_event: F,
    ) -> bool {
        let option_ids = self.nodes[select_id]
            .element_data()
            .and_then(|element| element.select_data())
            .map(|select| {
                select
                    .options
                    .iter()
                    .enumerate()
                    .map(|(index, option)| (index, option.node_id, option.disabled))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let mut changed = false;
        for (index, option_id, disabled) in option_ids {
            if disabled {
                continue;
            }
            let should_select = indices.contains(&index);
            if let Some(option_data) = self.nodes[option_id]
                .element_data_mut()
                .and_then(|element| element.option_data_mut())
            {
                if option_data.selected != should_select {
                    option_data.selected = should_select;
                    self.changed_nodes.insert(option_id);
                    changed = true;
                }
            }
        }

        if let Some(select) = self.nodes[select_id]
            .element_data_mut()
            .and_then(|element| element.select_data_mut())
        {
            select.active_index = indices.first().copied().or(select.active_index);
            select.anchor_index = indices.first().copied().or(select.anchor_index);
        }

        if changed {
            self.changed_nodes.insert(select_id);
            let value = self.primary_select_value(select_id).unwrap_or_default();
            dispatch_event(DomEvent::new(
                select_id,
                DomEventData::Input(BlitzInputEvent {
                    value: value.clone(),
                }),
            ));
            dispatch_event(DomEvent::new(
                select_id,
                DomEventData::Change(BlitzInputEvent { value }),
            ));
            self.shell_provider.request_redraw();
        }

        changed
    }

    pub(crate) fn activate_select_index<F: FnMut(DomEvent)>(
        &mut self,
        select_id: usize,
        index: usize,
        dispatch_event: F,
    ) -> bool {
        let mode = self.nodes[select_id]
            .element_data()
            .and_then(|element| element.select_data())
            .map(|select| select.mode)
            .unwrap_or(SelectMode::Dropdown);
        match mode {
            SelectMode::Dropdown => self.set_select_indices(select_id, &[index], dispatch_event),
            SelectMode::Listbox => {
                let mut indices = self.selected_option_indices(select_id);
                if indices.contains(&index) {
                    indices.retain(|value| *value != index);
                } else {
                    indices.push(index);
                    indices.sort_unstable();
                }
                self.set_select_indices(select_id, &indices, dispatch_event)
            }
        }
    }

    pub(crate) fn move_select_active(&mut self, select_id: usize, delta: isize) -> bool {
        let start = self.nodes[select_id]
            .element_data()
            .and_then(|element| element.select_data())
            .and_then(|select| select.active_index)
            .or_else(|| self.selected_option_indices(select_id).into_iter().next())
            .unwrap_or(0);
        let Some(select) = self.nodes[select_id]
            .element_data_mut()
            .and_then(|element| element.select_data_mut())
        else {
            return false;
        };
        if select.options.is_empty() {
            return false;
        }

        let mut current = start as isize;
        for _ in 0..select.options.len() {
            current = (current + delta).clamp(0, select.options.len() as isize - 1);
            let candidate = current as usize;
            if !select.options[candidate].disabled {
                let changed = select.active_index != Some(candidate);
                select.active_index = Some(candidate);
                if changed {
                    self.changed_nodes.insert(select_id);
                    self.shell_provider.request_redraw();
                }
                return changed;
            }
        }
        false
    }

    pub(crate) fn set_select_active(&mut self, select_id: usize, index: usize) -> bool {
        let Some(select) = self.nodes[select_id]
            .element_data_mut()
            .and_then(|element| element.select_data_mut())
        else {
            return false;
        };
        if index >= select.options.len() || select.options[index].disabled {
            return false;
        }
        let changed = select.active_index != Some(index);
        select.active_index = Some(index);
        if changed {
            self.changed_nodes.insert(select_id);
            self.shell_provider.request_redraw();
        }
        changed
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SelectPopupBounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[cfg(test)]
mod tests {
    use crate::{Attribute, DocumentConfig, QualName, ns, qual_name};
    use markup5ever::LocalName;

    use super::*;

    fn attr(name: &str, value: &str) -> Attribute {
        Attribute {
            name: QualName {
                prefix: None,
                ns: ns!(),
                local: LocalName::from(name),
            },
            value: value.to_string(),
        }
    }

    fn bool_attr(name: &str) -> Attribute {
        attr(name, "")
    }

    fn append_option(
        doc: &mut BaseDocument,
        parent_id: usize,
        attrs: Vec<Attribute>,
        label: &str,
    ) -> usize {
        let mut mutator = doc.mutate();
        let option_id = mutator.create_element(qual_name!("option"), attrs);
        let text_id = mutator.create_text_node(label);
        mutator.append_children(option_id, &[text_id]);
        mutator.append_children(parent_id, &[option_id]);
        option_id
    }

    #[test]
    fn sync_select_control_prefers_controlled_value() {
        let mut document = BaseDocument::new(DocumentConfig::default());
        let root_id = document.root_node().id;

        let select_id = {
            let mut mutator = document.mutate();
            let select_id =
                mutator.create_element(qual_name!("select"), vec![attr("value", "banana")]);
            mutator.append_children(root_id, &[select_id]);
            select_id
        };

        let first_option = append_option(
            &mut document,
            select_id,
            vec![attr("value", "apple"), bool_attr("selected")],
            "Apple",
        );
        let second_option = append_option(
            &mut document,
            select_id,
            vec![attr("value", "banana")],
            "Banana",
        );

        document.sync_select_control(select_id);

        let first_selected = document
            .get_node(first_option)
            .and_then(|node| node.element_data())
            .and_then(|element| element.option_data())
            .is_some_and(|data| data.selected);
        let second_selected = document
            .get_node(second_option)
            .and_then(|node| node.element_data())
            .and_then(|element| element.option_data())
            .is_some_and(|data| data.selected);

        assert!(!first_selected);
        assert!(second_selected);
        assert_eq!(
            document.primary_select_value(select_id).as_deref(),
            Some("banana")
        );
    }

    #[test]
    fn form_event_values_include_selected_enabled_options() {
        let mut document = BaseDocument::new(DocumentConfig::default());
        let root_id = document.root_node().id;

        let (form_id, select_id, disabled_group_id) = {
            let mut mutator = document.mutate();
            let form_id = mutator.create_element(qual_name!("form"), vec![attr("id", "form")]);
            let select_id = mutator.create_element(
                qual_name!("select"),
                vec![attr("name", "fruit"), bool_attr("multiple")],
            );
            let disabled_group_id =
                mutator.create_element(qual_name!("optgroup"), vec![bool_attr("disabled")]);
            mutator.append_children(root_id, &[form_id]);
            mutator.append_children(form_id, &[select_id]);
            mutator.append_children(select_id, &[disabled_group_id]);
            (form_id, select_id, disabled_group_id)
        };

        append_option(
            &mut document,
            select_id,
            vec![attr("value", "apple"), bool_attr("selected")],
            "Apple",
        );
        append_option(
            &mut document,
            select_id,
            vec![
                attr("value", "pear"),
                bool_attr("selected"),
                bool_attr("disabled"),
            ],
            "Pear",
        );
        append_option(
            &mut document,
            disabled_group_id,
            vec![attr("value", "plum"), bool_attr("selected")],
            "Plum",
        );

        document.sync_select_control(select_id);
        document.reset_form_owner(select_id);

        let values = document.form_event_values(select_id);

        assert_eq!(form_id, document.controls_to_form[&select_id]);
        assert_eq!(values, vec![(String::from("fruit"), String::from("apple"))]);
    }
}
